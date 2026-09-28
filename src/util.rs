//! Small Win32 helpers and the log file.

use std::ffi::OsStr;
use std::io::Write;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::Mutex;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, SYSTEMTIME};
use windows_sys::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows_sys::Win32::System::SystemInformation::GetLocalTime;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// NUL-terminated UTF-16 string for Win32 calls.
pub fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(std::iter::once(0)).collect()
}

pub fn exe_dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(PathBuf::from)).unwrap_or_else(|| PathBuf::from("."))
}

pub fn is_elevated() -> bool {
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        );
        CloseHandle(token);
        ok != 0 && elevation.TokenIsElevated != 0
    }
}

static LOG: Mutex<Option<std::fs::File>> = Mutex::new(None);

pub fn log_path() -> PathBuf {
    crate::config::Config::path().with_file_name("lueur.log")
}

/// Starts a fresh log file (resident mode only; CLI commands print instead).
pub fn init_log() {
    let path = log_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut file) = std::fs::File::create(path) {
        // BOM so that every Windows tool reads the accents correctly.
        let _ = file.write_all("\u{feff}".as_bytes());
        *LOG.lock().unwrap() = Some(file);
    }
}

pub fn log(msg: &str) {
    let mut guard = LOG.lock().unwrap();
    if let Some(file) = guard.as_mut() {
        let mut t: SYSTEMTIME = unsafe { std::mem::zeroed() };
        unsafe { GetLocalTime(&mut t) };
        let _ = writeln!(
            file,
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}  {msg}",
            t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
        );
    } else {
        eprintln!("{msg}");
    }
}

#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => { $crate::util::log(&format!($($arg)*)) };
}
