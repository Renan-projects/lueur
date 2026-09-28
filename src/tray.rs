//! Notification-area icon and its menu. The thread spends its life blocked in
//! `GetMessageW`; hardware work happens on a short-lived worker thread.

use lueur::config::Config;
use lueur::devices::{self, Report};
use lueur::effect::{Color, Effect, Speed};
use lueur::util::{self, is_elevated, wide};
use lueur::{autostart, log};
use std::cell::RefCell;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Storage::FileSystem::{
    FindFirstChangeNotificationW, FindNextChangeNotification, FILE_NOTIFY_CHANGE_FILE_NAME,
    FILE_NOTIFY_CHANGE_LAST_WRITE,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{CreateMutexW, WaitForSingleObject, INFINITE};
use windows_sys::Win32::UI::Controls::Dialogs::{ChooseColorW, CC_FULLOPEN, CC_RGBINIT, CHOOSECOLORW};
use windows_sys::Win32::UI::Shell::{
    ShellExecuteW, Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIIF_WARNING, NIM_ADD,
    NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const CLASS_NAME: &str = "LueurTray";
const WM_TRAY: u32 = WM_APP + 1;
const WM_APPLIED: u32 = WM_APP + 2;
const WM_CONFIG_CHANGED: u32 = WM_APP + 3;
/// Sent by a second `lueur.exe` launch: reapply and show that we are alive.
const WM_WAKE: u32 = WM_APP + 4;

const TIMER_RESUME: usize = 1;
const TIMER_RELOAD: usize = 2;

const PBT_APMRESUMESUSPEND: usize = 0x7;
const PBT_APMRESUMEAUTOMATIC: usize = 0x12;

const ID_EFFECT: usize = 100;
const ID_SPEED: usize = 200;
const ID_BRIGHTNESS: usize = 300;
const ID_COLOR: usize = 400;
const ID_REAPPLY: usize = 401;
const ID_PERSIST: usize = 402;
const ID_AUTOSTART: usize = 403;
const ID_EDIT_CONFIG: usize = 404;
const ID_OPEN_LOG: usize = 405;
const ID_QUIT: usize = 406;

const BRIGHTNESS_LEVELS: [u8; 5] = [100, 75, 50, 25, 10];

struct State {
    hwnd: HWND,
    icon: HICON,
    taskbar_created: u32,
    cfg: Config,
    zones: Vec<String>,
    notes: Vec<String>,
    applying: bool,
    /// A new apply was requested while one was running (value: persist).
    queued: Option<bool>,
    custom_colors: [u32; 16],
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(s.borrow_mut().as_mut().expect("état non initialisé")))
}

pub fn run(no_elevate: bool) {
    // One resident instance per session: a second launch just pokes the first.
    let mutex = unsafe { CreateMutexW(std::ptr::null(), 1, wide("Local\\Lueur.Resident").as_ptr()) };
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        let hwnd = unsafe { FindWindowW(wide(CLASS_NAME).as_ptr(), std::ptr::null()) };
        if !hwnd.is_null() {
            unsafe { PostMessageW(hwnd, WM_WAKE, 0, 0) };
        }
        return;
    }

    if !no_elevate && !is_elevated() {
        unsafe { CloseHandle(mutex) };
        // The scheduled task starts us elevated without a UAC prompt; otherwise ask once.
        if autostart::is_enabled() && autostart::run_task().is_ok() {
            return;
        }
        let exe = std::env::current_exe().unwrap_or_default();
        let r = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                wide("runas").as_ptr(),
                wide(exe.as_os_str()).as_ptr(),
                wide("").as_ptr(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        if r as isize > 32 {
            return;
        }
        // UAC refused: carry on without SMBus (USB devices still work).
        run_resident();
        return;
    }
    run_resident();
    unsafe { CloseHandle(mutex) };
}

fn run_resident() {
    util::init_log();
    log!("Lueur {} démarré (admin : {})", env!("CARGO_PKG_VERSION"), is_elevated());

    let (cfg, cfg_error) = match Config::load() {
        Ok(c) => (c, None),
        Err(e) => (Config::default(), Some(e)),
    };

    unsafe {
        let hinstance = GetModuleHandleW(std::ptr::null());
        let class = wide(CLASS_NAME);
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance,
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassW(&wc);
        // A hidden top-level window (not message-only): only those receive
        // power broadcasts and the "TaskbarCreated" message.
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            wide("Lueur").as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        );
        // Let a non-elevated second launch reach this elevated window.
        ChangeWindowMessageFilterEx(hwnd, WM_WAKE, MSGFLT_ALLOW, std::ptr::null_mut());

        let mut icon = LoadImageW(
            hinstance,
            1 as _,
            IMAGE_ICON,
            GetSystemMetrics(SM_CXSMICON),
            GetSystemMetrics(SM_CYSMICON),
            LR_DEFAULTCOLOR,
        ) as HICON;
        if icon.is_null() {
            icon = LoadIconW(std::ptr::null_mut(), IDI_APPLICATION);
        }

        STATE.with(|s| {
            *s.borrow_mut() = Some(State {
                hwnd,
                icon,
                taskbar_created: RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()),
                cfg,
                zones: Vec::new(),
                notes: Vec::new(),
                applying: false,
                queued: None,
                custom_colors: [0x00FF_FFFF; 16],
            })
        });

        add_icon();
        if let Some(e) = cfg_error {
            log!("configuration illisible, valeurs par défaut utilisées : {e}");
            notify("Erreur dans config.toml", &e, true);
        }
        watch_config(hwnd);
        request_apply(false);

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    log!("Lueur arrêté");
}

fn icon_data() -> NOTIFYICONDATAW {
    with_state(|s| {
        let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = s.hwnd;
        nid.uID = 1;
        nid
    })
}

fn copy_wide(dst: &mut [u16], text: &str) {
    let src: Vec<u16> = text.encode_utf16().take(dst.len() - 1).collect();
    dst[..src.len()].copy_from_slice(&src);
    dst[src.len()] = 0;
}

fn tooltip() -> String {
    with_state(|s| {
        let mut t = format!("Lueur — {}", s.cfg.effect.label());
        if s.cfg.effect.uses_color() {
            t.push_str(&format!(" {}", s.cfg.color));
        }
        t
    })
}

fn add_icon() {
    let mut nid = icon_data();
    nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    nid.uCallbackMessage = WM_TRAY;
    nid.hIcon = with_state(|s| s.icon);
    copy_wide(&mut nid.szTip, &tooltip());
    unsafe { Shell_NotifyIconW(NIM_ADD, &nid) };
}

fn update_tip() {
    let mut nid = icon_data();
    nid.uFlags = NIF_TIP;
    copy_wide(&mut nid.szTip, &tooltip());
    unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };
}

fn notify(title: &str, text: &str, warning: bool) {
    let mut nid = icon_data();
    nid.uFlags = NIF_INFO;
    nid.dwInfoFlags = if warning { NIIF_WARNING } else { NIIF_INFO };
    copy_wide(&mut nid.szInfoTitle, title);
    copy_wide(&mut nid.szInfo, text);
    unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };
}

/// Applies the current configuration on a worker thread; requests arriving
/// meanwhile are merged into a single follow-up run.
fn request_apply(persist: bool) {
    let job = with_state(|s| {
        if s.applying {
            s.queued = Some(s.queued.unwrap_or(false) || persist);
            return None;
        }
        s.applying = true;
        Some((s.cfg.clone(), s.hwnd as isize))
    });
    let Some((cfg, hwnd)) = job else { return };
    std::thread::spawn(move || {
        let report = Box::new(devices::apply(&cfg, persist));
        unsafe { PostMessageW(hwnd as HWND, WM_APPLIED, usize::from(persist), Box::into_raw(report) as LPARAM) };
    });
}

fn on_applied(report: Report, persisted: bool) {
    for w in &report.warnings {
        log!("avertissement : {w}");
    }
    for e in &report.errors {
        log!("erreur : {e}");
    }
    log!("{} zone(s) pilotée(s){}", report.zones.len(), if persisted { ", enregistré dans le matériel" } else { "" });

    let queued = with_state(|s| {
        s.applying = false;
        s.zones = report.zones.iter().map(|(id, name)| format!("{name}   [{id}]")).collect();
        s.notes = report.errors.iter().chain(&report.warnings).cloned().collect();
        s.queued.take()
    });
    if !report.errors.is_empty() {
        notify("Lueur : problème avec un périphérique", &report.errors.join("\n"), true);
    } else if persisted {
        notify("Lueur", "Réglage enregistré dans la mémoire des contrôleurs.", false);
    }
    if let Some(p) = queued {
        request_apply(p);
    }
}

/// Wakes the UI thread whenever something in the configuration folder is written.
fn watch_config(hwnd: HWND) {
    let Some(dir) = Config::path().parent().map(|d| d.to_path_buf()) else { return };
    let hwnd = hwnd as isize;
    std::thread::spawn(move || unsafe {
        let h = FindFirstChangeNotificationW(
            wide(dir.as_os_str()).as_ptr(),
            0,
            FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_FILE_NAME,
        );
        if h == INVALID_HANDLE_VALUE {
            return;
        }
        while WaitForSingleObject(h, INFINITE) == WAIT_OBJECT_0 {
            PostMessageW(hwnd as HWND, WM_CONFIG_CHANGED, 0, 0);
            if FindNextChangeNotification(h) == 0 {
                break;
            }
        }
    });
}

fn reload_config() {
    match Config::load() {
        Ok(new) => {
            let changed = with_state(|s| {
                let changed = s.cfg != new;
                s.cfg = new;
                changed
            });
            if changed {
                log!("configuration rechargée");
                update_tip();
                request_apply(false);
            }
        }
        Err(e) => {
            log!("configuration illisible : {e}");
            notify("Erreur dans config.toml", &e, true);
        }
    }
}

/// Changes the configuration from the menu, saves it and applies it.
fn change(f: impl FnOnce(&mut Config)) {
    let result = with_state(|s| {
        f(&mut s.cfg);
        s.cfg.save()
    });
    if let Err(e) = result {
        notify("Lueur", &e, true);
    }
    update_tip();
    request_apply(false);
}

fn pick_color() {
    let (hwnd, initial, mut custom) = with_state(|s| (s.hwnd, s.cfg.color, s.custom_colors));
    let mut cc: CHOOSECOLORW = unsafe { std::mem::zeroed() };
    cc.lStructSize = std::mem::size_of::<CHOOSECOLORW>() as u32;
    cc.hwndOwner = hwnd;
    cc.rgbResult = initial.to_colorref();
    cc.lpCustColors = custom.as_mut_ptr();
    cc.Flags = CC_FULLOPEN | CC_RGBINIT;
    let ok = unsafe { ChooseColorW(&mut cc) } != 0;
    with_state(|s| s.custom_colors = custom);
    if ok {
        change(|c| {
            c.color = Color::from_colorref(cc.rgbResult);
            if !c.effect.uses_color() {
                c.effect = Effect::Static;
            }
        });
    }
}

fn open_in_notepad(path: &std::path::Path) {
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            wide("open").as_ptr(),
            wide("notepad.exe").as_ptr(),
            wide(format!("\"{}\"", path.display())).as_ptr(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
}

unsafe fn append(menu: HMENU, flags: u32, id: usize, text: &str) {
    AppendMenuW(menu, flags, id, wide(text).as_ptr());
}

fn check(on: bool) -> u32 {
    if on {
        MF_CHECKED
    } else {
        MF_UNCHECKED
    }
}

fn show_menu() {
    let (hwnd, cfg, zones, notes, applying) =
        with_state(|s| (s.hwnd, s.cfg.clone(), s.zones.clone(), s.notes.clone(), s.applying));
    let autostart_on = autostart::is_enabled();
    unsafe {
        let menu = CreatePopupMenu();
        let radio = MFT_RADIOCHECK | MF_STRING;

        let effects = CreatePopupMenu();
        for (i, e) in Effect::ALL.iter().enumerate() {
            if *e == Effect::Off {
                append(effects, MF_SEPARATOR, 0, "");
            }
            append(effects, radio | check(*e == cfg.effect), ID_EFFECT + i, e.label());
        }
        append(menu, MF_POPUP, effects as usize, &format!("Effet : {}", cfg.effect.label()));

        append(menu, MF_STRING, ID_COLOR, &format!("Couleur… ({})", cfg.color));

        let brightness = CreatePopupMenu();
        for (i, b) in BRIGHTNESS_LEVELS.iter().enumerate() {
            append(brightness, radio | check(*b == cfg.brightness), ID_BRIGHTNESS + i, &format!("{b} %"));
        }
        append(menu, MF_POPUP, brightness as usize, &format!("Luminosité : {} %", cfg.brightness));

        let speeds = CreatePopupMenu();
        for (i, sp) in Speed::ALL.iter().enumerate() {
            append(speeds, radio | check(*sp == cfg.speed), ID_SPEED + i, sp.label());
        }
        append(menu, MF_POPUP, speeds as usize, &format!("Vitesse (RAM) : {}", cfg.speed.label()));

        append(menu, MF_SEPARATOR, 0, "");
        let devices_menu = CreatePopupMenu();
        if zones.is_empty() {
            let text = if applying { "Détection en cours…" } else { "Aucun périphérique détecté" };
            append(devices_menu, MF_STRING | MF_GRAYED, 0, text);
        }
        for z in &zones {
            append(devices_menu, MF_STRING | MF_GRAYED, 0, z);
        }
        if !notes.is_empty() {
            append(devices_menu, MF_SEPARATOR, 0, "");
            for n in &notes {
                append(devices_menu, MF_STRING | MF_GRAYED, 0, &format!("⚠ {n}"));
            }
        }
        append(menu, MF_POPUP, devices_menu as usize, &format!("Périphériques ({})", zones.len()));
        append(menu, MF_STRING, ID_REAPPLY, "Réappliquer");
        append(menu, MF_STRING, ID_PERSIST, "Enregistrer dans le matériel");

        append(menu, MF_SEPARATOR, 0, "");
        append(menu, MF_STRING | check(autostart_on), ID_AUTOSTART, "Lancer au démarrage de Windows");
        append(menu, MF_STRING, ID_EDIT_CONFIG, "Modifier la configuration…");
        append(menu, MF_STRING, ID_OPEN_LOG, "Ouvrir le journal");
        append(menu, MF_SEPARATOR, 0, "");
        append(menu, MF_STRING, ID_QUIT, "Quitter");

        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        SetForegroundWindow(hwnd);
        let cmd =
            TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY, pt.x, pt.y, 0, hwnd, std::ptr::null());
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);
        on_command(cmd as usize, autostart_on);
    }
}

fn on_command(cmd: usize, autostart_on: bool) {
    match cmd {
        0 => {}
        c if (ID_EFFECT..ID_EFFECT + Effect::ALL.len()).contains(&c) => {
            change(|cfg| cfg.effect = Effect::ALL[c - ID_EFFECT])
        }
        c if (ID_SPEED..ID_SPEED + Speed::ALL.len()).contains(&c) => change(|cfg| cfg.speed = Speed::ALL[c - ID_SPEED]),
        c if (ID_BRIGHTNESS..ID_BRIGHTNESS + BRIGHTNESS_LEVELS.len()).contains(&c) => {
            change(|cfg| cfg.brightness = BRIGHTNESS_LEVELS[c - ID_BRIGHTNESS])
        }
        ID_COLOR => pick_color(),
        ID_REAPPLY => request_apply(false),
        ID_PERSIST => request_apply(true),
        ID_AUTOSTART => {
            let r = if autostart_on { autostart::disable() } else { autostart::enable() };
            if let Err(e) = r {
                notify("Lancement au démarrage", &e, true);
            }
        }
        ID_EDIT_CONFIG => open_in_notepad(&Config::path()),
        ID_OPEN_LOG => open_in_notepad(&util::log_path()),
        ID_QUIT => unsafe {
            let nid = icon_data();
            Shell_NotifyIconW(NIM_DELETE, &nid);
            PostQuitMessage(0);
        },
        _ => {}
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TRAY => {
            let event = (lparam & 0xFFFF) as u32;
            if event == WM_RBUTTONUP || event == WM_LBUTTONUP || event == WM_CONTEXTMENU {
                show_menu();
            }
            0
        }
        WM_APPLIED => {
            let report = Box::from_raw(lparam as *mut Report);
            on_applied(*report, wparam != 0);
            0
        }
        WM_CONFIG_CHANGED => {
            SetTimer(hwnd, TIMER_RELOAD, 300, None);
            0
        }
        WM_WAKE => {
            request_apply(false);
            notify("Lueur", "Lueur est déjà lancé (icône dans la zone de notification).", false);
            0
        }
        WM_POWERBROADCAST => {
            if wparam == PBT_APMRESUMEAUTOMATIC || wparam == PBT_APMRESUMESUSPEND {
                // Controllers need a moment after resume before they listen.
                SetTimer(hwnd, TIMER_RESUME, 4000, None);
            }
            1
        }
        WM_TIMER => {
            KillTimer(hwnd, wparam);
            match wparam {
                TIMER_RESUME => {
                    log!("sortie de veille : réapplication");
                    request_apply(false);
                }
                TIMER_RELOAD => reload_config(),
                _ => {}
            }
            0
        }
        _ => {
            let taskbar_created = STATE.with(|s| s.borrow().as_ref().map(|s| s.taskbar_created));
            if taskbar_created == Some(msg) {
                add_icon();
                return 0;
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
    }
}
