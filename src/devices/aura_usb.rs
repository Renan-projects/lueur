//! ASUS Aura USB motherboard controllers (AULA3-* firmware): 12 V RGB headers,
//! onboard LEDs and addressable (ARGB) headers.
//!
//! Protocol documented by the OpenRGB project
//! (Controllers/AsusAuraUSBController, GPL-2.0-or-later).

use super::{Device, Zone};
use crate::effect::{Effect, Lighting};
use crate::hid;
use std::time::Duration;

const VID: u16 = 0x0B05;
const MAINBOARD_PIDS: [u16; 5] = [0x18F3, 0x1939, 0x19AF, 0x1AA6, 0x1BED];
const USAGE_PAGE: u16 = 0xFF72;

const REPORT_ID: u8 = 0xEC;
const REQ_FIRMWARE: u8 = 0x82;
const REQ_CONFIG_TABLE: u8 = 0xB0;
const CMD_EFFECT: u8 = 0x35;
const CMD_EFFECT_COLOR: u8 = 0x36;
const CMD_COMMIT: u8 = 0x3F;

struct Channel {
    /// Effect channel number on the controller.
    effect: u8,
    /// Index of the channel's first LED in the effect color mask.
    first_led: u8,
    leds: u8,
    zone: &'static str,
    label: String,
}

pub struct AuraMainboard {
    dev: hid::Device,
    pid: u16,
    firmware: String,
    channels: Vec<Channel>,
}

pub fn detect(out: &mut Vec<Box<dyn Device>>, warnings: &mut Vec<String>) {
    for found in hid::find(VID, &MAINBOARD_PIDS) {
        let pid = found.pid;
        let dev = match hid::Device::open(&found.path) {
            Ok(d) if d.usage_page == USAGE_PAGE => d,
            Ok(_) => continue,
            Err(e) => {
                warnings.push(format!("ASUS Aura USB {pid:04X} : {e}"));
                continue;
            }
        };
        match AuraMainboard::init(dev, pid) {
            Ok(board) => out.push(Box::new(board)),
            Err(e) => warnings.push(format!("ASUS Aura USB {pid:04X} : {e}")),
        }
    }
}

impl AuraMainboard {
    fn init(dev: hid::Device, pid: u16) -> Result<Self, String> {
        let mut board = AuraMainboard { dev, pid, firmware: String::new(), channels: Vec::new() };

        let fw = board.request(REQ_FIRMWARE, 0x02)?;
        board.firmware =
            fw[2..18].iter().take_while(|&&c| c != 0).map(|&c| char::from(c)).collect::<String>().trim().to_owned();

        let reply = board.request(REQ_CONFIG_TABLE, 0x30)?;
        let table = &reply[4..64];
        let onboard_leds = table[0x1B];
        let mut rgb_headers = table[0x1D];
        let argb_headers = table[0x02];
        if onboard_leds < rgb_headers {
            rgb_headers = 0;
        }

        let mut effect = 0u8;
        if onboard_leds > 0 {
            let label = if rgb_headers == onboard_leds {
                format!("{rgb_headers} en-tête(s) RGB 12 V")
            } else if rgb_headers > 0 {
                format!("LED de la carte + {rgb_headers} en-tête(s) RGB 12 V")
            } else {
                "LED de la carte".to_owned()
            };
            board.channels.push(Channel { effect, first_led: 0, leds: onboard_leds, zone: "rgb", label });
            effect += 1;
        }
        const ARGB_ZONES: [&str; 8] = ["argb1", "argb2", "argb3", "argb4", "argb5", "argb6", "argb7", "argb8"];
        let mut first_led = onboard_leds;
        for (i, zone) in ARGB_ZONES.iter().enumerate().take(usize::from(argb_headers)) {
            board
                .channels
                .push(Channel { effect, first_led, leds: 1, zone, label: format!("en-tête ARGB {}", i + 1) });
            effect += 1;
            first_led += 1;
        }

        // Switch the controller to the protocol generation used below.
        board.send(&[0x52, 0x53, 0x00, 0x01])?;
        Ok(board)
    }

    fn send(&self, payload: &[u8]) -> Result<(), String> {
        let mut buf = [0u8; 65];
        buf[0] = REPORT_ID;
        buf[1..=payload.len()].copy_from_slice(payload);
        self.dev.write(&buf)
    }

    /// Sends a request and waits for the reply whose code is `expect`.
    /// Returns the raw report: `[0xEC, code, ...]`.
    fn request(&self, cmd: u8, expect: u8) -> Result<[u8; 65], String> {
        let mut reply = [0u8; 65];
        while self.dev.read(&mut reply, Duration::ZERO)? > 0 {}
        self.send(&[cmd])?;
        for _ in 0..8 {
            if self.dev.read(&mut reply, Duration::from_millis(500))? == 0 {
                break;
            }
            if reply[0] == REPORT_ID && reply[1] == expect {
                return Ok(reply);
            }
        }
        Err(format!("pas de réponse à la requête 0x{cmd:02X}"))
    }

    fn id(&self) -> String {
        format!("aura-usb:{:04x}", self.pid)
    }
}

impl Device for AuraMainboard {
    fn name(&self) -> String {
        format!("Carte mère ASUS Aura ({})", self.firmware)
    }

    fn zones(&self) -> Vec<Zone> {
        self.channels.iter().map(|c| Zone { id: format!("{}/{}", self.id(), c.zone), name: c.label.clone() }).collect()
    }

    fn apply(&mut self, lighting: &dyn Fn(&str) -> Option<Lighting>, persist: bool) -> Result<(), String> {
        let id = self.id();
        let mut touched = false;
        for ch in &self.channels {
            let Some(l) = lighting(&format!("{id}/{}", ch.zone)) else { continue };
            touched = true;
            self.send(&[CMD_EFFECT, ch.effect, 0x00, 0x00, l.effect.hw_mode()])?;
            // The color mask is 16 bits wide: LEDs beyond it keep the effect's default color.
            if l.effect == Effect::Off || u32::from(ch.first_led) + u32::from(ch.leds) > 16 {
                continue;
            }
            let c = l.output_color();
            let mask: u16 = ((1u16 << ch.leds) - 1) << ch.first_led;
            let mut packet = [0u8; 64];
            packet[0] = CMD_EFFECT_COLOR;
            packet[1] = (mask >> 8) as u8;
            packet[2] = mask as u8;
            for led in ch.first_led..ch.first_led + ch.leds {
                let at = 4 + 3 * usize::from(led);
                packet[at..at + 3].copy_from_slice(&[c.r, c.g, c.b]);
            }
            self.send(&packet)?;
        }
        if persist && touched {
            self.send(&[CMD_COMMIT, 0x55])?;
        }
        Ok(())
    }
}
