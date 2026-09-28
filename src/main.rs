//! `lueur.exe`: the resident part, an icon in the notification area.
#![windows_subsystem = "windows"]

mod tray;

fn main() {
    let no_elevate = std::env::args().any(|a| a == "--no-elevate");
    tray::run(no_elevate);
}
