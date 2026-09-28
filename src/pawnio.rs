//! Minimal binding to PawnIOLib.dll (https://pawnio.eu), the signed kernel
//! driver used to reach the SMBus. The library is loaded at runtime so Lueur
//! still starts (USB devices only) when PawnIO is not installed.

use crate::util::{exe_dir, wide};
use std::ffi::CStr;
use std::path::PathBuf;
use std::sync::OnceLock;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

type FnOpen = unsafe extern "system" fn(*mut HANDLE) -> i32;
type FnLoad = unsafe extern "system" fn(HANDLE, *const u8, usize) -> i32;
type FnExecute = unsafe extern "system" fn(HANDLE, *const u8, *const u64, usize, *mut u64, usize, *mut usize) -> i32;
type FnClose = unsafe extern "system" fn(HANDLE) -> i32;
type FarProc = unsafe extern "system" fn() -> isize;

struct Lib {
    open: FnOpen,
    load: FnLoad,
    execute: FnExecute,
    close: FnClose,
}

pub const E_ACCESSDENIED: i32 = 0x8007_0005_u32 as i32;
pub const E_NOT_SUPPORTED: i32 = 0x8007_0032_u32 as i32;

static LIB: OnceLock<Result<Lib, String>> = OnceLock::new();

fn lib() -> Result<&'static Lib, String> {
    LIB.get_or_init(load_lib).as_ref().map_err(Clone::clone)
}

fn load_lib() -> Result<Lib, String> {
    let mut candidates = vec![exe_dir().join("PawnIOLib.dll")];
    if let Some(pf) = std::env::var_os("ProgramFiles") {
        candidates.push(PathBuf::from(pf).join("PawnIO").join("PawnIOLib.dll"));
    }
    for path in candidates {
        let module = unsafe { LoadLibraryW(wide(path.as_os_str()).as_ptr()) };
        if module.is_null() {
            continue;
        }
        let sym = |name: &CStr| unsafe { GetProcAddress(module, name.as_ptr().cast()) };
        let (Some(open), Some(load), Some(execute), Some(close)) =
            (sym(c"pawnio_open"), sym(c"pawnio_load"), sym(c"pawnio_execute"), sym(c"pawnio_close"))
        else {
            return Err(format!("{} incomplet", path.display()));
        };
        // SAFETY: signatures come from PawnIOLib.h (HRESULT STDAPICALLTYPE).
        unsafe {
            return Ok(Lib {
                open: std::mem::transmute::<FarProc, FnOpen>(open),
                load: std::mem::transmute::<FarProc, FnLoad>(load),
                execute: std::mem::transmute::<FarProc, FnExecute>(execute),
                close: std::mem::transmute::<FarProc, FnClose>(close),
            });
        }
    }
    Err("PawnIO n'est pas installé (https://pawnio.eu)".into())
}

/// An open PawnIO handle with one module loaded into it.
pub struct Module {
    handle: HANDLE,
}

// The handle is a plain kernel file handle, usable from any thread.
unsafe impl Send for Module {}

impl Module {
    pub fn load(blob: &[u8]) -> Result<Module, i32> {
        let lib = lib().map_err(|_| -1)?;
        let mut handle: HANDLE = std::ptr::null_mut();
        let hr = unsafe { (lib.open)(&mut handle) };
        if hr < 0 {
            return Err(hr);
        }
        let hr = unsafe { (lib.load)(handle, blob.as_ptr(), blob.len()) };
        if hr < 0 {
            unsafe { (lib.close)(handle) };
            return Err(hr);
        }
        Ok(Module { handle })
    }

    /// Runs one of the module's `ioctl_*` functions.
    pub fn execute(&self, name: &CStr, input: &[u64], output: &mut [u64]) -> Result<usize, i32> {
        let lib = lib().map_err(|_| -1)?;
        let mut returned = 0usize;
        let hr = unsafe {
            (lib.execute)(
                self.handle,
                name.as_ptr().cast(),
                input.as_ptr(),
                input.len(),
                output.as_mut_ptr(),
                output.len(),
                &mut returned,
            )
        };
        if hr < 0 {
            Err(hr)
        } else {
            Ok(returned)
        }
    }
}

impl Drop for Module {
    fn drop(&mut self) {
        if let Ok(lib) = lib() {
            unsafe { (lib.close)(self.handle) };
        }
    }
}

pub fn availability() -> Result<(), String> {
    lib().map(|_| ())
}
