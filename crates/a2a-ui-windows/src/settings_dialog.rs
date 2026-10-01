//! Settings dialog: the machine/environment values that used to be scattered
//! through Rust, PowerShell and Node defaults.
//!
//! Deliberately small: two groups (basic / advanced), plain `EDIT` fields, and a
//! save that can never send a message. Session bindings and handoff text stay in
//! their own dialogs — this page does not manage sessions.
use crate::dialog_theme::{self, DialogPalette};
use crate::settings::SettingsForm;
use crate::view;
use crate::win::{
    child_edit, child_static, dialog_button, hwnd_text, paint_dialog_button,
    subclass_dialog_button, warm_font, Fonts, CFG_BROWSER, CFG_CANCEL, CFG_CLAUDE_HOST,
    CFG_DATA_HOME, CFG_DELAY, CFG_ORIGIN, CFG_POLL, CFG_SAVE, CFG_TITLE_PATTERN, CFG_WORKSPACE,
};
use handoff_core::config::{self, RuntimeConfig};
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::*,
        Graphics::Gdi::{CreateSolidBrush, DeleteObject, HBRUSH, HFONT},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::{DRAWITEMSTRUCT, EM_SETLIMITTEXT},
            HiDpi::GetDpiForWindow,
            Input::KeyboardAndMouse::*,
            WindowsAndMessaging::*,
        },
    },
};

struct SettingsState {
    product: PathBuf,
    /// Loaded config, including any deprecated keys that were dropped on load.
    initial: RuntimeConfig,
    font: HFONT,
    fields: Vec<(usize, HWND)>,
    palette: DialogPalette,
    /// Font cache for the owner-drawn buttons, built at the dialog DPI on creation.
    button_fonts: Option<Fonts>,
    /// Dialog background, needed to blend the rounded button corners.
    background: HBRUSH,
    saved: bool,
}

impl SettingsState {
    fn new(product: PathBuf) -> Self {
        let initial = config::load(&product).config;
        Self {
            product,
            initial,
            font: HFONT(std::ptr::null_mut()),
            fields: Vec::new(),
            // Safety: the palette only creates two solid brushes.
            palette: unsafe { DialogPalette::new() },
            button_fonts: None,
            background: unsafe { CreateSolidBrush(colorref(dialog_theme::DIALOG_BACKGROUND)) },
            saved: false,
        }
    }
    fn field(&self, id: usize) -> HWND {
        self.fields
            .iter()
            .find(|(i, _)| *i == id)
            .map(|(_, h)| *h)
            .unwrap_or_default()
    }
    /// Collect the form into a RuntimeConfig, validating as we go.
    unsafe fn collect(&self) -> Result<RuntimeConfig, String> {
        let text = |id: usize| hwnd_text(self.field(id));
        let form = SettingsForm {
            dsh_data_home: text(CFG_DATA_HOME),
            dsh_web_origin: text(CFG_ORIGIN),
            poll_seconds: text(CFG_POLL),
            dispatch_delay_seconds: text(CFG_DELAY),
            dsh_browser_processes: text(CFG_BROWSER),
            workspace: text(CFG_WORKSPACE),
            claude_host: text(CFG_CLAUDE_HOST),
            dsh_page_title_pattern: text(CFG_TITLE_PATTERN),
        };
        // `apply` validates and normalizes; `initial` supplies the fields the form
        // does not own (notably `enabled`), so saving can never flip the switch.
        form.apply(&self.initial)
    }
}

fn colorref(argb: u32) -> COLORREF {
    COLORREF(((argb & 0xFF) << 16) | (argb & 0xFF00) | ((argb >> 16) & 0xFF))
}

unsafe fn settings_ptr(hwnd: HWND) -> *mut SettingsState {
    GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut SettingsState
}

/// One label + one single-line edit row, with an optional hint underneath.
/// Geometry is shared so the columns line up: label column, then field column.
const LABEL_X: i32 = 24;
const LABEL_W: i32 = 200;
const FIELD_X: i32 = 232;
const FIELD_W: i32 = 500;
const CONTENT_W: i32 = FIELD_X + FIELD_W - LABEL_X;

unsafe fn row(
    parent: HWND,
    id: usize,
    title: &str,
    hint: &str,
    value: &str,
    y: i32,
    dpi: u32,
    font: HFONT,
) -> HWND {
    // One line, vertically centred against the 34 DIP field.
    child_static(parent, title, LABEL_X, y + 7, LABEL_W, 24, dpi, font);
    let edit = child_edit(parent, id, value, FIELD_X, y, FIELD_W, 34, dpi, font);
    SendMessageW(edit, EM_SETLIMITTEXT, Some(WPARAM(600)), Some(LPARAM(0)));
    if !hint.is_empty() {
        child_static(parent, hint, FIELD_X, y + 37, FIELD_W, 24, dpi, font);
    }
    edit
}

unsafe extern "system" fn settings_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if let Some(p) = std::env::var_os("A2A_DLG_TRACE") {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p)
        {
            let _ = writeln!(f, "settings_proc msg={msg:#06x} wp={:#x}", wp.0);
        }
    }
    if msg == WM_NCCREATE {
        let cs = &*(lp.0 as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    let ptr = settings_ptr(hwnd);
    if ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    match msg {
        WM_CREATE => {
            let st = &mut *ptr;
            let dpi = GetDpiForWindow(hwnd).max(96);
            st.font = warm_font(dpi);
            // Built here, not in `new`, because the correct DPI is only known once
            // the dialog exists (a 96-DPI cache renders half-size text at 200%).
            st.button_fonts = Some(Fonts::new(dpi));
            let font = st.font;
            // Drive the rows from the form model so what is shown and what is saved
            // can never drift apart.
            let form = SettingsForm::from_config(&st.initial);
            let mut add = |id: usize, edit: HWND| st.fields.push((id, edit));

            child_static(hwnd, "基础设置", LABEL_X, 16, CONTENT_W, 26, dpi, font);
            let e = row(
                hwnd,
                CFG_DATA_HOME,
                "DSH 数据目录",
                "包含 storages/session_projcache/sessions 的目录；留空表示尚未配置。",
                &form.dsh_data_home,
                46,
                dpi,
                font,
            );
            add(CFG_DATA_HOME, e);
            let e = row(
                hwnd,
                CFG_ORIGIN,
                "DSH Web 地址",
                "例如 http://127.0.0.1:3080；用于确认目标页面，不会访问外网。",
                &form.dsh_web_origin,
                108,
                dpi,
                font,
            );
            add(CFG_ORIGIN, e);
            let e = row(
                hwnd,
                CFG_POLL,
                "自动轮询周期（秒）",
                "1–7200。",
                &form.poll_seconds,
                170,
                dpi,
                font,
            );
            add(CFG_POLL, e);
            let e = row(
                hwnd,
                CFG_DELAY,
                "发送倒计时（秒）",
                "1–120。草稿写入后等待这么久才提交，期间可取消。",
                &form.dispatch_delay_seconds,
                232,
                dpi,
                font,
            );
            add(CFG_DELAY, e);

            child_static(hwnd, "高级设置", LABEL_X, 300, CONTENT_W, 26, dpi, font);
            let e = row(
                hwnd,
                CFG_BROWSER,
                "DSH 浏览器进程",
                "逗号分隔，例如 msedge, chrome；留空则使用内置默认值。",
                &form.dsh_browser_processes,
                330,
                dpi,
                font,
            );
            add(CFG_BROWSER, e);
            let e = row(
                hwnd,
                CFG_WORKSPACE,
                "Workspace 校验",
                "留空表示关闭这项额外校验。",
                &form.workspace,
                392,
                dpi,
                font,
            );
            add(CFG_WORKSPACE, e);
            let e = row(
                hwnd,
                CFG_CLAUDE_HOST,
                "Claude Host",
                "Claude Cowork 文档所在域名，默认 claude.ai。",
                &form.claude_host,
                454,
                dpi,
                font,
            );
            add(CFG_CLAUDE_HOST, e);
            let e = row(
                hwnd,
                CFG_TITLE_PATTERN,
                "DSH 页面标题特征",
                "用于辅助确认 DSH 页面；会话身份仍以 Session ID 为准。",
                &form.dsh_page_title_pattern,
                516,
                dpi,
                font,
            );
            add(CFG_TITLE_PATTERN, e);

            child_static(
                hwnd,
                "保存只更新本机配置：不发送消息、不启动任务、不修改绑定与交接文案。",
                LABEL_X,
                580,
                CONTENT_W,
                30,
                dpi,
                st.font,
            );
            // Owner-drawn so the buttons match the main window's, not the light
            // system push button.
            for (id, label, x) in [(CFG_CANCEL, "取消", 384), (CFG_SAVE, "保存设置", 512)] {
                let b = dialog_button(hwnd, id, label, x, 618, 120, 42, dpi, font);
                subclass_dialog_button(b);
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = wp.0 & 0xffff;
            if id == CFG_CANCEL {
                let _ = DestroyWindow(hwnd);
                return LRESULT(0);
            }
            if id == CFG_SAVE {
                let st = &mut *ptr;
                match st.collect() {
                    Ok(next) => match config::save(&st.product, &next) {
                        Ok(()) => {
                            st.saved = true;
                            let _ = DestroyWindow(hwnd);
                        }
                        Err(e) => {
                            let m = wide(&format!("配置未保存：{e}"));
                            let _ = MessageBoxW(
                                Some(hwnd),
                                PCWSTR(m.as_ptr()),
                                w!("设置"),
                                MB_OK | MB_ICONWARNING,
                            );
                        }
                    },
                    Err(e) => {
                        let m = wide(&e);
                        let _ = MessageBoxW(
                            Some(hwnd),
                            PCWSTR(m.as_ptr()),
                            w!("设置"),
                            MB_OK | MB_ICONWARNING,
                        );
                    }
                }
                return LRESULT(0);
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_ERASEBKGND => {
            let st = &*ptr;
            dialog_theme::erase_background(hwnd, wp, &st.palette)
        }
        // Every text child uses the dialog palette so the form is dark like the
        // main window instead of the default system grey.
        msg if dialog_theme::is_ctlcolor(msg) => {
            let st = &*ptr;
            let child = HWND(lp.0 as *mut c_void);
            let is_input = st.fields.iter().any(|(_, h)| *h == child);
            dialog_theme::paint_child(msg, wp, &st.palette, is_input)
        }
        WM_DRAWITEM => {
            let st = &*ptr;
            let Some(fonts) = st.button_fonts.as_ref() else {
                return LRESULT(1);
            };
            let item = &*(lp.0 as *const DRAWITEMSTRUCT);
            paint_dialog_button(
                item,
                fonts,
                GetDpiForWindow(hwnd).max(96),
                crate::win::hover_slot(hwnd) == item.CtlID as usize,
                colorref(dialog_theme::DIALOG_BACKGROUND),
            );
            LRESULT(1)
        }
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            let st = &mut *ptr;
            if !st.font.is_invalid() {
                let _ = DeleteObject(st.font.into());
                st.font = HFONT(std::ptr::null_mut());
            }
            if !st.background.is_invalid() {
                let _ = DeleteObject(st.background.into());
                st.background = HBRUSH(std::ptr::null_mut());
            }
            LRESULT(0)
        }
        WM_NCDESTROY => {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Show the settings dialog. Returns Ok(true) when the user saved.
pub unsafe fn show(parent: HWND, product: &Path) -> Result<bool, String> {
    let instance = GetModuleHandleW(None).map_err(|e| e.to_string())?;
    let class = w!("A2AHandoff.Settings.v2");
    RegisterClassW(&WNDCLASSW {
        hCursor: LoadCursorW(None, IDC_ARROW).map_err(|e| e.to_string())?,
        hInstance: instance.into(),
        lpszClassName: class,
        lpfnWndProc: Some(settings_proc),
        ..Default::default()
    });
    let state = Box::new(SettingsState::new(product.to_owned()));
    let raw = Box::into_raw(state);
    let dpi = GetDpiForWindow(parent).max(96);
    let (x, y, width, height) = dialog_theme::centered_rect(view::px(780, dpi), view::px(720, dpi));
    let hwnd = match CreateWindowExW(
        WS_EX_DLGMODALFRAME,
        class,
        w!("A2AHandoff · 设置"),
        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_VISIBLE | WS_CLIPCHILDREN,
        x,
        y,
        width,
        height,
        Some(parent),
        None,
        Some(instance.into()),
        Some(raw as *const c_void),
    ) {
        Ok(h) => h,
        Err(e) => {
            drop(Box::from_raw(raw));
            return Err(e.to_string());
        }
    };
    dialog_theme::apply_dark_title_bar(hwnd);
    let _ = EnableWindow(parent, false);
    let _ = SetForegroundWindow(hwnd);
    let mut m = MSG::default();
    while IsWindow(Some(hwnd)).as_bool() {
        let status = GetMessageW(&mut m, None, 0, 0).0;
        if status <= 0 {
            break;
        }
        if !IsDialogMessageW(hwnd, &m).as_bool() {
            let _ = TranslateMessage(&m);
            let _ = DispatchMessageW(&m);
        }
    }
    let _ = EnableWindow(parent, true);
    let _ = SetForegroundWindow(parent);
    let state = Box::from_raw(raw);
    Ok(state.saved)
}
