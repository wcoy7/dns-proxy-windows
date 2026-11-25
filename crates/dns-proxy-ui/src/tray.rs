//! System tray icon + context menu for the control panel.
//!
//! Window hide/show uses Win32 `ShowWindow` instead of egui
//! `ViewportCommand::Visible`, which can strand the event loop on Windows
//! once the viewport is made invisible (egui#5229).

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use eframe::egui;
use tray_icon::menu::{Menu, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    ShowWindow, SW_HIDE, SW_RESTORE, SW_SHOW,
};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

pub const MENU_OPEN: &str = "open";
pub const MENU_START: &str = "start";
pub const MENU_STOP: &str = "stop";
pub const MENU_QUIT: &str = "quit";

/// Shared HWND + visibility flag used by tray handlers and the app update loop.
#[derive(Clone)]
pub struct WindowState {
    hwnd: Arc<AtomicIsize>,
    visible: Arc<AtomicBool>,
}

impl WindowState {
    pub fn new() -> Self {
        Self {
            hwnd: Arc::new(AtomicIsize::new(0)),
            visible: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn set_hwnd_from_cc(&self, cc: &eframe::CreationContext<'_>) {
        if let Ok(handle) = cc.window_handle() {
            if let RawWindowHandle::Win32(win) = handle.as_raw() {
                self.hwnd.store(win.hwnd.get() as isize, Ordering::SeqCst);
            }
        }
    }

    pub fn is_visible(&self) -> bool {
        self.visible.load(Ordering::SeqCst)
    }

    pub fn show(&self) {
        let hwnd = self.hwnd.load(Ordering::SeqCst);
        if hwnd == 0 {
            return;
        }
        let hwnd = HWND(hwnd as *mut _);
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        self.visible.store(true, Ordering::SeqCst);
    }

    pub fn hide(&self) {
        let hwnd = self.hwnd.load(Ordering::SeqCst);
        if hwnd == 0 {
            return;
        }
        let hwnd = HWND(hwnd as *mut _);
        unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
        self.visible.store(false, Ordering::SeqCst);
    }

}

pub struct TrayHandles {
    /// Must be kept alive for the lifetime of the app.
    pub _tray: TrayIcon,
    #[allow(dead_code)]
    pub open_item: MenuItem,
    #[allow(dead_code)]
    pub start_item: MenuItem,
    #[allow(dead_code)]
    pub stop_item: MenuItem,
    #[allow(dead_code)]
    pub quit_item: MenuItem,
}

pub fn build_tray(window: &WindowState, egui_ctx: egui::Context) -> Result<TrayHandles> {
    let icon = load_tray_icon().context("failed to load tray icon")?;

    let open_item = MenuItem::with_id(MENU_OPEN, "Open / Show", true, None);
    let start_item = MenuItem::with_id(MENU_START, "Start service", true, None);
    let stop_item = MenuItem::with_id(MENU_STOP, "Stop service", true, None);
    let quit_item = MenuItem::with_id(MENU_QUIT, "Quit", true, None);

    let menu = Menu::new();
    menu.append(&open_item)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&start_item)?;
    menu.append(&stop_item)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&quit_item)?;

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("DNS Proxy")
        .with_icon(icon)
        .build()
        .context("failed to create tray icon")?;

    // Wake the egui / winit loop so tray + menu events are processed promptly.
    let win_for_tray = window.clone();
    let ctx_tray = egui_ctx.clone();
    TrayIconEvent::set_event_handler(Some(move |event| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = event
        {
            win_for_tray.show();
        }
        ctx_tray.request_repaint();
    }));

    let ctx_menu = egui_ctx;
    tray_icon::menu::MenuEvent::set_event_handler(Some(move |_event| {
        ctx_menu.request_repaint();
    }));

    Ok(TrayHandles {
        _tray: tray,
        open_item,
        start_item,
        stop_item,
        quit_item,
    })
}

fn load_tray_icon() -> Result<Icon> {
    let candidates = [
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/dns-proxy-icon.png"),
        std::path::PathBuf::from("assets/dns-proxy-icon.png"),
    ];
    for path in candidates {
        if let Ok(bytes) = std::fs::read(&path) {
            let img = image::load_from_memory(&bytes)?.into_rgba8();
            let (w, h) = img.dimensions();
            return Ok(Icon::from_rgba(img.into_raw(), w, h)?);
        }
    }
    // Fallback: solid teal square so the tray still appears if assets are missing.
    let size = 32u32;
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for _ in 0..size * size {
        rgba.extend_from_slice(&[0x14, 0xB8, 0xA6, 0xFF]);
    }
    Ok(Icon::from_rgba(rgba, size, size)?)
}
