//! ENE SMBus controllers found on "Aura compatible" RGB memory
//! (ADATA XPG Spectrix, G.Skill Trident Z, Geil, Team T-Force…).
//!
//! Register protocol documented by the OpenRGB project
//! (Controllers/ENESMBusController, GPL-2.0-or-later).

use super::{Device, Zone};
use crate::effect::{Effect, Lighting};
use crate::smbus::Bus;
use std::time::Duration;

const REG_DEVICE_NAME: u16 = 0x1000;
const REG_MICRON_CHECK: u16 = 0x1030;
const REG_CONFIG_TABLE: u16 = 0x1C00;
const REG_COLORS_EFFECT_V1: u16 = 0x8010;
const REG_COLORS_EFFECT_V2: u16 = 0x8160;
const REG_DIRECT: u16 = 0x8020;
const REG_MODE: u16 = 0x8021;
const REG_SPEED: u16 = 0x8022;
const REG_DIRECTION: u16 = 0x8023;
const REG_APPLY: u16 = 0x80A0;
const REG_SLOT_INDEX: u16 = 0x80F8;
const REG_I2C_ADDRESS: u16 = 0x80F9;

const APPLY: u8 = 0x01;
const SAVE: u8 = 0xAA;

/// Address every ENE DRAM controller answers on after power-up, before being
/// given its own address.
const DEFAULT_ADDRESS: u8 = 0x77;

/// Addresses we give to (or find) RAM controllers on. 0x40/0x4E/0x4F are left
/// out on purpose: ASUS boards put their own ENE controller there.
const RAM_ADDRESSES: [u8; 12] = [0x39, 0x3A, 0x3B, 0x3C, 0x3D, 0x70, 0x71, 0x72, 0x73, 0x75, 0x76, 0x77];

/// Firmware names we know the register layout of. Anything else is skipped:
/// writing to the wrong registers of an unknown chip is not worth the risk.
fn effect_register(firmware: &str) -> Option<u16> {
    match firmware {
        "LED-0116" | "DIMM_LED-0102" | "DIMM_LED-0103" | "AUMA0-E8K4-0101" => Some(REG_COLORS_EFFECT_V1),
        "AUDA0-E6K5-0101" | "AUMA0-E6K5-0104" | "AUMA0-E6K5-0105" | "AUMA0-E6K5-0106" | "AUMA0-E6K5-0107" => {
            Some(REG_COLORS_EFFECT_V2)
        }
        _ => None,
    }
}

pub fn detect(bus: Bus, out: &mut Vec<Box<dyn Device>>, warnings: &mut Vec<String>) {
    let bus = std::rc::Rc::new(bus);
    let _g = bus.lock();

    assign_addresses(&bus);

    for addr in RAM_ADDRESSES {
        if !is_ene(&bus, addr) {
            continue;
        }
        let firmware = read_string(&bus, addr, REG_DEVICE_NAME);
        let Some(effect_reg) = effect_register(&firmware) else {
            warnings.push(format!("RAM ENE 0x{addr:02X} : firmware « {firmware} » inconnu, ignorée par sécurité"));
            continue;
        };
        let mut table = [0u8; 4];
        for (i, b) in table.iter_mut().enumerate() {
            *b = read_reg(&bus, addr, REG_CONFIG_TABLE + i as u16).unwrap_or(0);
        }
        let leds = if firmware == "AUMA0-E6K5-0107" { table[3] } else { table[2] };
        if leds == 0 || leds > 16 {
            warnings.push(format!("RAM ENE 0x{addr:02X} : nombre de LED incohérent ({leds}), ignorée"));
            continue;
        }
        out.push(Box::new(EneDram { bus: bus.clone(), addr, firmware, leds, effect_reg }));
    }
}

/// After a cold boot, all sticks share address 0x77. Each one is moved to a
/// free address by writing its slot number and the new address; the stick in
/// that slot answers on the new address from then on (until power loss).
fn assign_addresses(bus: &Bus) {
    for slot in 0..8u8 {
        if bus.read_byte(DEFAULT_ADDRESS).is_none() || !is_ene(bus, DEFAULT_ADDRESS) {
            return;
        }
        let Some(free) = RAM_ADDRESSES[..RAM_ADDRESSES.len() - 1].iter().copied().find(|&a| bus.read_byte(a).is_none())
        else {
            return;
        };
        write_reg(bus, DEFAULT_ADDRESS, REG_SLOT_INDEX, slot);
        write_reg(bus, DEFAULT_ADDRESS, REG_I2C_ADDRESS, free << 1);
        std::thread::sleep(Duration::from_millis(1));
        crate::log!("RAM : emplacement {slot} → adresse 0x{free:02X} demandée");
    }
}

/// ENE chips mirror 0x00..0x0F in registers 0xA0..0xAF; Micron sticks use the
/// same chip without RGB and must be skipped.
fn is_ene(bus: &Bus, addr: u8) -> bool {
    if bus.read_byte(addr).is_none() && bus.read_byte_data(addr, 0x00).is_none() {
        return false;
    }
    let pattern = (0..16u8).all(|i| bus.read_byte_data(addr, 0xA0 + i) == Some(i));
    pattern && !read_string(bus, addr, REG_MICRON_CHECK).starts_with("Micron")
}

fn select(bus: &Bus, addr: u8, reg: u16) -> bool {
    bus.write_word_data(addr, 0x00, reg.swap_bytes())
}

fn read_reg(bus: &Bus, addr: u8, reg: u16) -> Option<u8> {
    if !select(bus, addr, reg) {
        return None;
    }
    bus.read_byte_data(addr, 0x81)
}

fn write_reg(bus: &Bus, addr: u8, reg: u16, value: u8) -> bool {
    select(bus, addr, reg) && bus.write_byte_data(addr, 0x01, value)
}

fn write_block(bus: &Bus, addr: u8, reg: u16, data: &[u8]) -> bool {
    if !select(bus, addr, reg) {
        return false;
    }
    bus.write_block_data(addr, 0x03, data) || data.iter().all(|&b| bus.write_byte_data(addr, 0x01, b))
}

fn read_string(bus: &Bus, addr: u8, reg: u16) -> String {
    (0..16u16).map_while(|i| read_reg(bus, addr, reg + i).filter(|&c| c != 0)).map(char::from).collect()
}

struct EneDram {
    bus: std::rc::Rc<Bus>,
    addr: u8,
    firmware: String,
    leds: u8,
    effect_reg: u16,
}

impl EneDram {
    fn id(&self) -> String {
        format!("ene-dram:{}:0x{:02x}", self.bus.name, self.addr)
    }
}

impl Device for EneDram {
    fn name(&self) -> String {
        format!("RAM RGB 0x{:02X} ({}, {} LED)", self.addr, self.firmware, self.leds)
    }

    fn zones(&self) -> Vec<Zone> {
        vec![Zone { id: self.id(), name: "barrette".into() }]
    }

    fn apply(&mut self, lighting: &dyn Fn(&str) -> Option<Lighting>, persist: bool) -> Result<(), String> {
        let Some(l) = lighting(&self.id()) else { return Ok(()) };
        let (bus, addr) = (&*self.bus, self.addr);
        let _g = bus.lock();

        let c = if l.effect == Effect::Off { Default::default() } else { l.output_color() };
        // ENE stores colors as R, B, G.
        let colors: Vec<u8> = (0..self.leds).flat_map(|_| [c.r, c.b, c.g]).collect();

        let mut ok = write_reg(bus, addr, REG_DIRECT, 0) && write_reg(bus, addr, REG_APPLY, APPLY);
        ok &= write_reg(bus, addr, REG_MODE, l.effect.hw_mode())
            && write_reg(bus, addr, REG_SPEED, l.speed.ene())
            && write_reg(bus, addr, REG_DIRECTION, u8::from(l.reverse));
        for (i, chunk) in colors.chunks(3).enumerate() {
            ok &= write_block(bus, addr, self.effect_reg + (i * 3) as u16, chunk);
        }
        ok &= write_reg(bus, addr, REG_APPLY, APPLY);
        if persist {
            ok &= write_reg(bus, addr, REG_APPLY, SAVE);
        }
        if ok {
            Ok(())
        } else {
            Err("échec d'écriture sur le SMBus".into())
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn register_select_is_big_endian_on_the_wire() {
        // SMBus words go low byte first: the high byte of the register must be sent first.
        assert_eq!(0x8021u16.swap_bytes(), 0x2180);
    }

    #[test]
    fn known_firmwares() {
        assert_eq!(super::effect_register("AUDA0-E6K5-0101"), Some(super::REG_COLORS_EFFECT_V2));
        assert_eq!(super::effect_register("???"), None);
    }
}
