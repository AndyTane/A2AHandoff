//! Shared appearance for the three modal dialogs (settings, binding config,
//! handoff text), so they read as the same product as the main window.
//!
//! Standard Win32 controls cannot be rounded or anti-aliased without rewriting
//! them, but the palette, fonts, focus behaviour and button rendering can match
//! exactly. The main window's tokens live in `view::tok`; the two colours below are
//! the dialog equivalents already used by `template_dialog.rs`.
use std::ffi::c_void;
use windows::Win32::{
    Foundation::*,
    Graphics::{
        Dwm::{DwmSetWindowAttribute, DWMWA_CAPTION_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE},
        Gdi::{CreateSolidBrush, DeleteObject, FillRect, SetBkColor, SetTextColor, HBRUSH, HDC},
    },
    UI::WindowsAndMessaging::*,
};

/// Dialog background (`0x0C121D`), a touch darker than the main window ground.
pub const DIALOG_BACKGROUND: u32 = 0x0C121D;
/// Input field background (`0x0D1725`).
pub const DIALOG_INPUT: u32 = 0x0D1725;
/// Primary text colour, matching the main window's `TEXT`.
pub const DIALOG_TEXT: u32 = 0xF0F5FB;

fn rgb(hex: u32) -> COLORREF {
    COLORREF(((hex & 0xFF) << 16) | (hex & 0xFF00) | ((hex >> 16) & 0xFF))
}

/// Brushes a dialog must keep alive for as long as its controls exist.
pub struct DialogPalette {
    pub background: HBRUSH,
    pub input: HBRUSH,
}
impl DialogPalette {
    pub unsafe fn new() -> Self {
        Self {
            background: CreateSolidBrush(rgb(DIALOG_BACKGROUND)),
            input: CreateSolidBrush(rgb(DIALOG_INPUT)),
        }
    }
}
impl Drop for DialogPalette {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.background.into());
            let _ = DeleteObject(self.input.into());
        }
    }
}

/// Give the dialog's title bar the same dark treatment as the main window.
pub unsafe fn apply_dark_title_bar(hwnd: HWND) {
    let on = 1i32;
    let _ = DwmSetWindowAttribute(
        hwnd,
        DWMWA_USE_IMMERSIVE_DARK_MODE,
        &on as *const _ as *const c_void,
        4,
    );
    let caption = rgb(DIALOG_BACKGROUND);
    let _ = DwmSetWindowAttribute(
        hwnd,
        DWMWA_CAPTION_COLOR,
        &caption as *const _ as *const c_void,
        4,
    );
}

/// True when `msg` is one of the control-colour notifications.
pub fn is_ctlcolor(msg: u32) -> bool {
    matches!(
        msg,
        WM_CTLCOLOREDIT | WM_CTLCOLORSTATIC | WM_CTLCOLORLISTBOX | WM_CTLCOLORBTN
    )
}

/// Paint an edit/static/listbox child in the dialog palette. `is_input` selects the
/// field background instead of the dialog background.
///
/// NOTE: a themed push button ignores WM_CTLCOLORBTN — verified on Windows 11 — so
/// dialog buttons are owner-drawn instead; see `paint_dialog_button`.
pub unsafe fn paint_child(
    msg: u32,
    wp: WPARAM,
    palette: &DialogPalette,
    is_input: bool,
) -> LRESULT {
    let hdc = HDC(wp.0 as *mut c_void);
    SetTextColor(hdc, rgb(DIALOG_TEXT));
    SetBkColor(
        hdc,
        rgb(if is_input {
            DIALOG_INPUT
        } else {
            DIALOG_BACKGROUND
        }),
    );
    // A listbox always takes the dialog brush; edits take the input brush.
    let brush = if is_input && msg != WM_CTLCOLORLISTBOX {
        palette.input
    } else {
        palette.background
    };
    LRESULT(brush.0 as isize)
}

/// Fill the whole client area with the dialog background.
pub unsafe fn erase_background(hwnd: HWND, wp: WPARAM, palette: &DialogPalette) -> LRESULT {
    let mut rc = RECT::default();
    let _ = GetClientRect(hwnd, &mut rc);
    FillRect(HDC(wp.0 as *mut c_void), &rc, palette.background);
    LRESULT(1)
}

/// Centred, work-area-clamped rectangle for a dialog of `w`x`h` device pixels.
/// The main window is only 960x480 DIP, so dialogs centre on the screen.
pub unsafe fn centered_rect(w: i32, h: i32) -> (i32, i32, i32, i32) {
    let mut work = RECT::default();
    let _ = SystemParametersInfoW(
        SPI_GETWORKAREA,
        0,
        Some(&mut work as *mut _ as *mut c_void),
        SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
    );
    let width = w.min(work.right - work.left);
    let height = h.min(work.bottom - work.top);
    let x = (work.left + (work.right - work.left - width) / 2).max(work.left);
    let y = (work.top + (work.bottom - work.top - height) / 2).max(work.top);
    (x, y, width, height)
}
