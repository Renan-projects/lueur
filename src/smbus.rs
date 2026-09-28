//! SMBus access through PawnIO's signed chipset modules (AMD PIIX4 / FCH and
//! Intel i801). Every transfer holds the system-wide SMBus mutex shared by
//! HWiNFO, OpenRGB, iCUE and others, so we never collide with them.

use crate::pawnio::{self, Module};
use crate::util::{exe_dir, wide};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0};
use windows_sys::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};

const READ: u64 = 1;
const WRITE: u64 = 0;
const PROTO_BYTE: u64 = 1;
const PROTO_BYTE_DATA: u64 = 2;
const PROTO_WORD_DATA: u64 = 3;
const PROTO_BLOCK_DATA: u64 = 5;

/// PawnIO's "always sleep" mode: waits yield the CPU instead of spinning.
const SLEEP_MODE_ALWAYS_SLEEP: u64 = 2;

pub struct Bus {
    /// Short stable name used in device IDs, e.g. `piix4` or `i801`.
    pub name: &'static str,
    module: Module,
    mutex: HANDLE,
}

unsafe impl Send for Bus {}

pub struct Guard<'a>(&'a Bus, bool);

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        if self.1 {
            unsafe { ReleaseMutex(self.0.mutex) };
        }
    }
}

impl Bus {
    /// Opens every SMBus controller PawnIO can drive on this machine.
    pub fn open_all(warnings: &mut Vec<String>) -> Vec<Bus> {
        if let Err(e) = pawnio::availability() {
            warnings.push(format!("SMBus (RAM) indisponible : {e}"));
            return Vec::new();
        }
        let mut buses = Vec::new();
        for (file, name, piix4_port) in [("SmbusPIIX4.bin", "piix4", Some(0u64)), ("SmbusI801.bin", "i801", None)] {
            let path = exe_dir().join(file);
            let Ok(blob) = std::fs::read(&path) else {
                warnings.push(format!("module PawnIO introuvable : {}", path.display()));
                continue;
            };
            let module = match Module::load(&blob) {
                Ok(m) => m,
                // The module refuses to load when the chipset is not the right one.
                Err(pawnio::E_NOT_SUPPORTED) => continue,
                Err(pawnio::E_ACCESSDENIED) => {
                    warnings.push("SMBus (RAM) : droits administrateur nécessaires".into());
                    return buses;
                }
                Err(hr) => {
                    crate::log!("module {file} non chargé (0x{:08X})", hr as u32);
                    continue;
                }
            };
            let bus = Bus { name, module, mutex: global_smbus_mutex() };
            {
                let _g = bus.lock();
                let _ = bus.module.execute(c"ioctl_set_sleep_mode", &[SLEEP_MODE_ALWAYS_SLEEP], &mut []);
                if let Some(port) = piix4_port {
                    let mut previous = [0u64; 1];
                    if bus.module.execute(c"ioctl_piix4_port_sel", &[port], &mut previous).is_err() {
                        warnings.push("sélection du port SMBus 0 impossible".into());
                        continue;
                    }
                }
            }
            buses.push(bus);
        }
        buses
    }

    /// Holds the global SMBus mutex. Win32 mutexes are recursive, so a device
    /// driver can hold it across a multi-transfer register access while each
    /// transfer locks it again.
    pub fn lock(&self) -> Guard<'_> {
        if self.mutex.is_null() {
            return Guard(self, false);
        }
        let r = unsafe { WaitForSingleObject(self.mutex, 2000) };
        Guard(self, r == WAIT_OBJECT_0 || r == WAIT_ABANDONED)
    }

    fn xfer(&self, addr: u8, rw: u64, cmd: u8, proto: u64, data: [u64; 5]) -> Option<[u64; 5]> {
        let _g = self.lock();
        let input = [u64::from(addr), rw, u64::from(cmd), proto, data[0], data[1], data[2], data[3], data[4]];
        let mut out = [0u64; 5];
        self.module.execute(c"ioctl_smbus_xfer", &input, &mut out).ok().map(|_| out)
    }

    /// Plain "receive byte": the usual presence probe.
    pub fn read_byte(&self, addr: u8) -> Option<u8> {
        self.xfer(addr, READ, 0, PROTO_BYTE, [0; 5]).map(|o| o[0] as u8)
    }

    pub fn read_byte_data(&self, addr: u8, cmd: u8) -> Option<u8> {
        self.xfer(addr, READ, cmd, PROTO_BYTE_DATA, [0; 5]).map(|o| o[0] as u8)
    }

    pub fn write_byte_data(&self, addr: u8, cmd: u8, value: u8) -> bool {
        self.xfer(addr, WRITE, cmd, PROTO_BYTE_DATA, [u64::from(value), 0, 0, 0, 0]).is_some()
    }

    pub fn write_word_data(&self, addr: u8, cmd: u8, value: u16) -> bool {
        self.xfer(addr, WRITE, cmd, PROTO_WORD_DATA, [u64::from(value), 0, 0, 0, 0]).is_some()
    }

    /// SMBus block write (at most 32 bytes).
    pub fn write_block_data(&self, addr: u8, cmd: u8, bytes: &[u8]) -> bool {
        debug_assert!(bytes.len() <= 32);
        self.xfer(addr, WRITE, cmd, PROTO_BLOCK_DATA, pack_block(bytes)).is_some()
    }
}

/// PawnIO block layout: length byte then data, packed little-endian into u64 cells.
fn pack_block(bytes: &[u8]) -> [u64; 5] {
    let mut raw = [0u8; 40];
    raw[0] = bytes.len() as u8;
    raw[1..=bytes.len()].copy_from_slice(bytes);
    let mut cells = [0u64; 5];
    for (cell, chunk) in cells.iter_mut().zip(raw.chunks_exact(8)) {
        *cell = u64::from_le_bytes(chunk.try_into().unwrap());
    }
    cells
}

fn global_smbus_mutex() -> HANDLE {
    unsafe { CreateMutexW(std::ptr::null(), 0, wide("Global\\Access_SMBUS.HTP.Method").as_ptr()) }
}

impl Drop for Bus {
    fn drop(&mut self) {
        if !self.mutex.is_null() {
            unsafe { CloseHandle(self.mutex) };
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn block_packing() {
        let cells = super::pack_block(&[0xAA, 0xBB, 0xCC]);
        assert_eq!(cells[0], 0xCC_BB_AA_03);
        assert_eq!(&cells[1..], &[0, 0, 0, 0]);
    }
}
