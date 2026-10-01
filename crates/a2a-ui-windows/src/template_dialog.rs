//! Modal editor for both directions; it never enqueues a handoff.
use super::*;
use crate::model::{load_templates, save_templates, MessageTemplates};
use handoff_core::{message_template::MAX_TEMPLATE_UNITS, Direction};
const TARGET: usize = 9201;
const PREFIX: usize = 9202;
const SUFFIX: usize = 9203;
const PREVIEW: usize = 9204;
const RESET: usize = 9290;
const SAVE: usize = 9291;
const CANCEL: usize = 9292;
struct Editor {
    product: PathBuf,
    value: MessageTemplates,
    direction: usize,
    combo: HWND,
    prefix: HWND,
    suffix: HWND,
    prefix_label: HWND,
    suffix_label: HWND,
    preview: HWND,
    note: HWND,
    font: HFONT,
    bg: HBRUSH,
    input: HBRUSH,
    raised: HBRUSH,
    /// Font cache for the owner-drawn buttons, built at the dialog DPI.
    button_fonts: Option<Fonts>,
    loading: bool,
    saved: bool,
}
impl Drop for Editor {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.font.into());
            let _ = DeleteObject(self.bg.into());
            let _ = DeleteObject(self.input.into());
            let _ = DeleteObject(self.raised.into());
        }
    }
}
unsafe fn raw_text(h: HWND) -> String {
    let n = GetWindowTextLengthW(h).max(0) as usize;
    let mut b = vec![0u16; n + 1];
    let k = GetWindowTextW(h, &mut b).max(0) as usize;
    String::from_utf16_lossy(&b[..k]).replace("\r\n", "\n")
}
unsafe fn set_text(h: HWND, s: &str) {
    let b = wide(&s.replace("\r\n", "\n").replace('\n', "\r\n"));
    let _ = SetWindowTextW(h, PCWSTR(b.as_ptr()));
}
unsafe fn multiline(
    parent: HWND,
    id: usize,
    x: i32,
    y: i32,
    wid: i32,
    hei: i32,
    dpi: u32,
    font: HFONT,
    readonly: bool,
) -> HWND {
    let h = CreateWindowExW(
        WS_EX_CLIENTEDGE,
        w!("EDIT"),
        w!(""),
        WS_CHILD
            | WS_VISIBLE
            | WS_TABSTOP
            | WS_VSCROLL
            | WINDOW_STYLE(
                (ES_MULTILINE
                    | ES_AUTOVSCROLL
                    | ES_WANTRETURN
                    | if readonly { ES_READONLY } else { 0 }) as u32,
            ),
        view::px(x, dpi),
        view::px(y, dpi),
        view::px(wid, dpi),
        view::px(hei, dpi),
        Some(parent),
        Some(HMENU(id as *mut c_void)),
        Some(GetModuleHandleW(None).unwrap_or_default().into()),
        None,
    )
    .unwrap_or_default();
    set_font(h, font);
    SendMessageW(
        h,
        EM_SETLIMITTEXT,
        Some(WPARAM(if readonly {
            100_000
        } else {
            MAX_TEMPLATE_UNITS
        })),
        Some(LPARAM(0)),
    );
    h
}
impl Editor {
    unsafe fn capture(&mut self) {
        let a = if self.direction == 0 {
            &mut self.value.to_dsh
        } else {
            &mut self.value.to_claude
        };
        a.prefix = raw_text(self.prefix);
        a.suffix = raw_text(self.suffix);
    }
    unsafe fn refresh_preview(&mut self) {
        self.capture();
        let dir = if self.direction == 0 {
            Direction::ClaudeToDsh
        } else {
            Direction::DshToClaude
        };
        let body = if self.direction == 0 {
            ""
        } else {
            "这里显示 DSH 最新完整回复，正文保持原样。"
        };
        set_text(self.preview, &self.value.render(dir, "", body, 7));
    }
    unsafe fn load_direction(&mut self) {
        self.loading = true;
        set_text(
            self.prefix_label,
            if self.direction == 0 {
                "执行指令 [文件名在这里填写]"
            } else {
                "回复前文 [末尾可写 [ 或 <]"
            },
        );
        set_text(
            self.suffix_label,
            if self.direction == 0 {
                "补充说明 [可留空]"
            } else {
                "回复后文 [开头可写 ] 或 >]"
            },
        );
        let a = if self.direction == 0 {
            &self.value.to_dsh
        } else {
            &self.value.to_claude
        };
        set_text(self.prefix, &a.prefix);
        set_text(self.suffix, &a.suffix);
        self.loading = false;
        self.refresh_preview();
    }
}
unsafe extern "system" fn editor_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let cs = &*(lp.0 as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Editor;
    if p.is_null() {
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    match msg {
        WM_CREATE => {
            let st = &mut *p;
            let dpi = GetDpiForWindow(hwnd).max(96);
            // The dialog DPI is only known here, so the button font cache is built now.
            st.button_fonts = Some(Fonts::new(dpi));
            child_static(hwnd, "配置方向", 20, 19, 88, 28, dpi, st.font);
            st.combo = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("COMBOBOX"),
                w!(""),
                WS_CHILD
                    | WS_VISIBLE
                    | WS_TABSTOP
                    | WS_VSCROLL
                    | WINDOW_STYLE(CBS_DROPDOWNLIST as u32),
                view::px(116, dpi),
                view::px(14, dpi),
                view::px(248, dpi),
                view::px(160, dpi),
                Some(hwnd),
                Some(HMENU(TARGET as *mut c_void)),
                Some(GetModuleHandleW(None).unwrap_or_default().into()),
                None,
            )
            .unwrap_or_default();
            set_font(st.combo, st.font);
            for label in ["发给 DSH [只发指令]", "发给 Claude [DSH 回复]"] {
                let s = wide(label);
                SendMessageW(
                    st.combo,
                    CB_ADDSTRING,
                    Some(WPARAM(0)),
                    Some(LPARAM(s.as_ptr() as isize)),
                );
            }
            SendMessageW(st.combo, CB_SETCURSEL, Some(WPARAM(0)), Some(LPARAM(0)));
            child_static(
                hwnd,
                "两方向独立配置；保存不会发送消息。",
                386,
                19,
                430,
                28,
                dpi,
                st.font,
            );
            st.prefix_label =
                child_static(hwnd, "执行指令 [文件名可改]", 20, 62, 386, 28, dpi, st.font);
            st.prefix = multiline(hwnd, PREFIX, 20, 92, 386, 112, dpi, st.font, false);
            st.suffix_label =
                child_static(hwnd, "补充说明 [可留空]", 20, 216, 386, 28, dpi, st.font);
            st.suffix = multiline(hwnd, SUFFIX, 20, 246, 386, 124, dpi, st.font, false);
            child_static(
                hwnd,
                "发送内容预览 [不会发送]",
                428,
                62,
                386,
                28,
                dpi,
                st.font,
            );
            st.preview = multiline(hwnd, PREVIEW, 428, 92, 386, 278, dpi, st.font, true);
            child_static(
                hwnd,
                "DSH 只收配置指令；Claude 收 DSH 正文及前后文。不添加交接标识。",
                20,
                386,
                794,
                26,
                dpi,
                st.font,
            );
            child_static(
                hwnd,
                "回复可用 [ ] 或 < > 包裹，在前后文中修改。文件名直接写进执行指令。",
                20,
                414,
                794,
                26,
                dpi,
                st.font,
            );
            st.note = child_static(
                hwnd,
                "保存后用于后续新投递；历史回执不会改变。",
                204,
                464,
                322,
                28,
                dpi,
                st.font,
            );
            // Every button is owner-drawn so the dialog matches the main window; the
            // leftover native one was the last light button on a dark surface.
            {
                let b = crate::win::dialog_button(
                    hwnd,
                    RESET,
                    "恢复本方向默认",
                    20,
                    456,
                    168,
                    42,
                    dpi,
                    st.font,
                );
                crate::win::subclass_dialog_button(b);
            }
            for (id, label, x, w) in [(CANCEL, "取消", 542, 120), (SAVE, "保存文案", 682, 132)]
            {
                let b = crate::win::dialog_button(hwnd, id, label, x, 456, w, 42, dpi, st.font);
                crate::win::subclass_dialog_button(b);
            }
            st.load_direction();
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = wp.0 & 0xffff;
            let code = (wp.0 >> 16) & 0xffff;
            let st = &mut *p;
            if id == TARGET && code == CBN_SELCHANGE as usize {
                st.capture();
                st.direction = SendMessageW(st.combo, CB_GETCURSEL, None, None)
                    .0
                    .clamp(0, 1) as usize;
                st.load_direction();
                return LRESULT(0);
            }
            if [PREFIX, SUFFIX].contains(&id) && code == EN_CHANGE as usize && !st.loading {
                st.refresh_preview();
                set_text(st.note, "尚未保存；预览不会发送。");
                return LRESULT(0);
            }
            if code == BN_CLICKED as usize {
                match id {
                    CANCEL | 2 => {
                        let _ = DestroyWindow(hwnd);
                        return LRESULT(0);
                    }
                    RESET => {
                        let defaults = MessageTemplates::default();
                        if st.direction == 0 {
                            st.value.to_dsh = defaults.to_dsh
                        } else {
                            st.value.to_claude = defaults.to_claude
                        }
                        st.load_direction();
                        set_text(st.note, "已恢复本方向默认，尚未保存。");
                        return LRESULT(0);
                    }
                    SAVE => {
                        st.capture();
                        match save_templates(&st.product, &st.value) {
                            Ok(()) => {
                                st.saved = true;
                                let _ = DestroyWindow(hwnd);
                            }
                            Err(e) => {
                                let s = wide(&e);
                                let _ = MessageBoxW(
                                    Some(hwnd),
                                    PCWSTR(s.as_ptr()),
                                    w!("保存文案失败"),
                                    MB_OK | MB_ICONWARNING,
                                );
                            }
                        }
                        return LRESULT(0);
                    }
                    _ => {}
                }
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_ERASEBKGND => {
            let mut rc = RECT::default();
            let _ = GetClientRect(hwnd, &mut rc);
            FillRect(HDC(wp.0 as *mut c_void), &rc, (*p).bg);
            LRESULT(1)
        }
        // Buttons take the main window's raised surface so this dialog is not
        // punctuated by light system buttons.
        WM_CTLCOLORBTN => {
            let st = &*p;
            let h = HDC(wp.0 as *mut c_void);
            SetTextColor(h, rgb(0xF0F5FB));
            SetBkColor(h, rgb(tok::RAISED));
            LRESULT(st.raised.0 as isize)
        }
        WM_CTLCOLOREDIT | WM_CTLCOLORSTATIC | WM_CTLCOLORLISTBOX => {
            let st = &*p;
            let child = HWND(lp.0 as *mut c_void);
            let edit = [st.prefix, st.suffix, st.preview, st.combo].contains(&child)
                || msg == WM_CTLCOLORLISTBOX;
            let h = HDC(wp.0 as *mut c_void);
            SetTextColor(h, rgb(0xF0F5FB));
            SetBkColor(h, rgb(if edit { INPUT } else { BACKGROUND }));
            LRESULT(if edit {
                st.input.0 as isize
            } else {
                st.bg.0 as isize
            })
        }
        WM_DRAWITEM => {
            let st = &*p;
            let Some(fonts) = st.button_fonts.as_ref() else {
                return LRESULT(1);
            };
            let item = &*(lp.0 as *const windows::Win32::UI::Controls::DRAWITEMSTRUCT);
            crate::win::paint_dialog_button(
                item,
                fonts,
                GetDpiForWindow(hwnd).max(96),
                crate::win::hover_slot(hwnd) == item.CtlID as usize,
                rgb(BACKGROUND),
            );
            LRESULT(1)
        }
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_NCDESTROY => {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
pub(super) unsafe fn show(parent: HWND, product: &Path) -> Result<bool, String> {
    let value = load_templates(product)?;
    let dpi = GetDpiForWindow(parent).max(96);
    let instance = GetModuleHandleW(None).map_err(|e| e.to_string())?;
    let class = w!("A2AHandoff.MessageTemplates.v1");
    RegisterClassW(&WNDCLASSW {
        hCursor: LoadCursorW(None, IDC_ARROW).map_err(|e| e.to_string())?,
        hInstance: instance.into(),
        lpszClassName: class,
        lpfnWndProc: Some(editor_proc),
        ..Default::default()
    });
    let font = CreateFontW(
        -view::px(17, dpi),
        0,
        0,
        0,
        400,
        0,
        0,
        0,
        DEFAULT_CHARSET,
        OUT_DEFAULT_PRECIS,
        CLIP_DEFAULT_PRECIS,
        CLEARTYPE_QUALITY,
        DEFAULT_PITCH.0 as u32,
        w!("Microsoft YaHei UI"),
    );
    let state = Box::new(Editor {
        product: product.to_owned(),
        value,
        direction: 0,
        combo: HWND::default(),
        prefix: HWND::default(),
        suffix: HWND::default(),
        prefix_label: HWND::default(),
        suffix_label: HWND::default(),
        preview: HWND::default(),
        note: HWND::default(),
        font,
        bg: CreateSolidBrush(rgb(BACKGROUND)),
        input: CreateSolidBrush(rgb(INPUT)),
        raised: CreateSolidBrush(rgb(tok::RAISED)),
        button_fonts: None,
        loading: true,
        saved: false,
    });
    let raw = Box::into_raw(state);
    let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_CLIPCHILDREN;
    let ex = WS_EX_DLGMODALFRAME;
    let mut size = RECT {
        left: 0,
        top: 0,
        right: view::px(834, dpi),
        bottom: view::px(516, dpi),
    };
    let _ = AdjustWindowRectExForDpi(&mut size, style, false, ex, dpi);
    let mut pr = RECT::default();
    let _ = GetWindowRect(parent, &mut pr);
    let mut work = RECT::default();
    let _ = SystemParametersInfoW(
        SPI_GETWORKAREA,
        0,
        Some(&mut work as *mut _ as *mut c_void),
        SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
    );
    let width = size.right - size.left;
    let height = size.bottom - size.top;
    let x = (pr.left + (pr.right - pr.left - width) / 2)
        .clamp(work.left, (work.right - width).max(work.left));
    let y = (pr.top + (pr.bottom - pr.top - height) / 2)
        .clamp(work.top, (work.bottom - height).max(work.top));
    let hwnd = match CreateWindowExW(
        ex,
        class,
        w!("A2AHandoff · 交接文案"),
        style,
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
    // One shared dark title-bar treatment for all three dialogs.
    crate::dialog_theme::apply_dark_title_bar(hwnd);
    let _ = EnableWindow(parent, false);
    let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
    let _ = SetForegroundWindow(hwnd);
    let mut m = MSG::default();
    while IsWindow(Some(hwnd)).as_bool() {
        let status = GetMessageW(&mut m, None, 0, 0).0;
        if status <= 0 {
            let _ = DestroyWindow(hwnd);
            if status == 0 {
                PostQuitMessage(m.wParam.0 as i32);
            }
            break;
        }
        if !IsDialogMessageW(hwnd, &m).as_bool() {
            let _ = TranslateMessage(&m);
            DispatchMessageW(&m);
        }
    }
    let _ = EnableWindow(parent, true);
    let _ = SetForegroundWindow(parent);
    let state = Box::from_raw(raw);
    Ok(state.saved)
}
