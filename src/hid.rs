//! Tiny native HID access. Devices are matched on the VID/PID embedded in
//! their interface path, so unrelated devices are never opened (some take
//! seconds to answer the string queries general-purpose HID libraries make).

use crate::util::wide;
use std::time::Duration;
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW, SetupDiGetDeviceInterfaceDetailW,
    DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W,
};
use windows_sys::Win32::Devices::HumanInterfaceDevice::{
    HidD_FreePreparsedData, HidD_GetHidGuid, HidD_GetPreparsedData, HidP_GetCaps, HIDP_CAPS, HIDP_STATUS_SUCCESS,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_IO_PENDING, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
    WAIT_OBJECT_0,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, WriteFile, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

pub struct Found {
    pub path: String,
    pub pid: u16,
}

/// Lists the HID interfaces of `vid` whose product ID is in `pids`.
pub fn find(vid: u16, pids: &[u16]) -> Vec<Found> {
    let mut found = Vec::new();
    unsafe {
        let mut guid = std::mem::zeroed();
        HidD_GetHidGuid(&mut guid);
        let set =
            SetupDiGetClassDevsW(&guid, std::ptr::null(), std::ptr::null_mut(), DIGCF_PRESENT | DIGCF_DEVICEINTERFACE);
        if set == INVALID_HANDLE_VALUE as _ {
            return found;
        }
        let mut index = 0;
        loop {
            let mut iface: SP_DEVICE_INTERFACE_DATA = std::mem::zeroed();
            iface.cbSize = std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32;
            if SetupDiEnumDeviceInterfaces(set, std::ptr::null(), &guid, index, &mut iface) == 0 {
                break;
            }
            index += 1;
            let mut size = 0u32;
            SetupDiGetDeviceInterfaceDetailW(set, &iface, std::ptr::null_mut(), 0, &mut size, std::ptr::null_mut());
            if size == 0 {
                continue;
            }
            // u64 storage keeps the structure correctly aligned.
            let mut storage = vec![0u64; (size as usize).div_ceil(8)];
            let detail = storage.as_mut_ptr().cast::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>();
            (*detail).cbSize = std::mem::size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
            if SetupDiGetDeviceInterfaceDetailW(set, &iface, detail, size, std::ptr::null_mut(), std::ptr::null_mut())
                == 0
            {
                continue;
            }
            let path_ptr = std::ptr::addr_of!((*detail).DevicePath).cast::<u16>();
            let len = (0..).take_while(|&i| *path_ptr.add(i) != 0).count();
            let path = String::from_utf16_lossy(std::slice::from_raw_parts(path_ptr, len));
            if let Some(pid) = parse_ids(&path).filter(|&(v, p)| v == vid && pids.contains(&p)).map(|(_, p)| p) {
                found.push(Found { path, pid });
            }
        }
        SetupDiDestroyDeviceInfoList(set);
    }
    found
}

/// Extracts VID and PID from a path like `\\?\hid#vid_0b05&pid_19af&mi_02#...`.
fn parse_ids(path: &str) -> Option<(u16, u16)> {
    let lower = path.to_ascii_lowercase();
    let hex = |key: &str| {
        let at = lower.find(key)? + key.len();
        u16::from_str_radix(lower.get(at..at + 4)?, 16).ok()
    };
    Some((hex("vid_")?, hex("pid_")?))
}

pub struct Device {
    handle: HANDLE,
    event: HANDLE,
    pub usage_page: u16,
    input_len: usize,
    output_len: usize,
}

impl Device {
    pub fn open(path: &str) -> Result<Device, String> {
        unsafe {
            let handle = CreateFileW(
                wide(path).as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                std::ptr::null_mut(),
            );
            if handle == INVALID_HANDLE_VALUE {
                return Err(format!("ouverture impossible (erreur {})", GetLastError()));
            }
            let event = CreateEventW(std::ptr::null(), 1, 0, std::ptr::null());
            let mut dev = Device { handle, event, usage_page: 0, input_len: 0, output_len: 0 };
            let mut pp = 0;
            if HidD_GetPreparsedData(handle, &mut pp) != 0 {
                let mut caps: HIDP_CAPS = std::mem::zeroed();
                if HidP_GetCaps(pp, &mut caps) == HIDP_STATUS_SUCCESS {
                    dev.usage_page = caps.UsagePage;
                    dev.input_len = usize::from(caps.InputReportByteLength);
                    dev.output_len = usize::from(caps.OutputReportByteLength);
                }
                HidD_FreePreparsedData(pp);
            }
            if event.is_null() || dev.output_len == 0 {
                return Err("capacités HID illisibles".into());
            }
            Ok(dev)
        }
    }

    /// Sends one output report (`data[0]` is the report ID); padded to the report size.
    pub fn write(&self, data: &[u8]) -> Result<(), String> {
        let mut buf = vec![0u8; self.output_len.max(data.len())];
        buf[..data.len()].copy_from_slice(data);
        self.io(false, &mut buf, Duration::from_secs(1)).map(|_| ()).map_err(|e| format!("écriture USB : {e}"))
    }

    /// Reads one input report into `buf`; returns 0 on timeout.
    pub fn read(&self, buf: &mut [u8], timeout: Duration) -> Result<usize, String> {
        let mut tmp = vec![0u8; self.input_len.max(1)];
        let n = self.io(true, &mut tmp, timeout).map_err(|e| format!("lecture USB : {e}"))?;
        let n = n.min(buf.len());
        buf[..n].copy_from_slice(&tmp[..n]);
        Ok(n)
    }

    fn io(&self, read: bool, buf: &mut [u8], timeout: Duration) -> Result<usize, String> {
        unsafe {
            let mut ov: OVERLAPPED = std::mem::zeroed();
            ov.hEvent = self.event;
            let ok = if read {
                ReadFile(self.handle, buf.as_mut_ptr(), buf.len() as u32, std::ptr::null_mut(), &mut ov)
            } else {
                WriteFile(self.handle, buf.as_ptr(), buf.len() as u32, std::ptr::null_mut(), &mut ov)
            };
            if ok == 0 && GetLastError() != ERROR_IO_PENDING {
                return Err(format!("erreur {}", GetLastError()));
            }
            let mut done = 0u32;
            if WaitForSingleObject(self.event, timeout.as_millis() as u32) != WAIT_OBJECT_0 {
                CancelIoEx(self.handle, &ov);
                GetOverlappedResult(self.handle, &ov, &mut done, 1);
                return if read { Ok(0) } else { Err("délai dépassé".into()) };
            }
            if GetOverlappedResult(self.handle, &ov, &mut done, 0) == 0 {
                return Err(format!("erreur {}", GetLastError()));
            }
            Ok(done as usize)
        }
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.event);
            CloseHandle(self.handle);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ids_from_path() {
        let p = r"\\?\HID#VID_0B05&PID_19AF&MI_02#8&1a2b3c&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}";
        assert_eq!(super::parse_ids(p), Some((0x0B05, 0x19AF)));
        assert_eq!(super::parse_ids(r"\\?\hid#foo"), None);
    }
}
