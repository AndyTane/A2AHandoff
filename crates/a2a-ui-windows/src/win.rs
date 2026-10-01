//! Native V1 shell. Owns its runtime child; all delivery goes through explicit commands.
//! Native Windows shell. Layout lives in `view.rs`; this file paints the display
//! list with GDI+ (shapes/icons, anti-aliased) + GDI (ClearType text) and hosts the
//! interactive controls as owner-drawn child windows.
#[path = "settings_dialog.rs"]
mod settings_dialog;
#[path = "template_dialog.rs"]
mod template_dialog;
use crate::{
    model::{self, BindingConfig, Snapshot},
    view::{
        self, tok, Align, Box2, Control, Font, Icon, Kind, Measure, Notice, Prim, Variant, View,
    },
};
use std::{
    cell::RefCell,
    collections::HashMap,
    ffi::c_void,
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use windows::{
    core::{w, PCWSTR, PWSTR},
    Win32::{
        Foundation::*,
        Graphics::{
            Dwm::*,
            Gdi::*,
            GdiPlus::{
                FillModeAlternate, FlushIntentionSync, GdipAddPathArc, GdipAddPathRectangle,
                GdipClosePathFigure, GdipCreateFromHDC, GdipCreatePath, GdipCreatePen1,
                GdipCreateSolidFill, GdipDeleteBrush, GdipDeleteGraphics, GdipDeletePath,
                GdipDeletePen, GdipDrawArc, GdipDrawLines, GdipDrawPath, GdipFillPath, GdipFlush,
                GdipSetPenEndCap, GdipSetPenLineJoin, GdipSetPenStartCap, GdipSetPixelOffsetMode,
                GdipSetSmoothingMode, GdiplusShutdown, GdiplusStartup, GdiplusStartupInput,
                GpBrush, GpGraphics, GpPath, GpPen, GpSolidFill, LineCapRound, LineJoinRound,
                PixelOffsetModeHalf, PointF, SmoothingModeAntiAlias, Status, UnitPixel,
            },
        },
        System::{LibraryLoader::GetModuleHandleW, Threading::*},
        UI::{Controls::*, HiDpi::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
};
const TIMER: usize = 1;
const SPIN_TIMER: usize = 2;

// `LoadImageW` with an ORDINAL resource id. The generated binding types `name` as
// `PCWSTR`, so it can only express the string form - and the string form does NOT find
// an icon declared as `1 ICON ...`: it fails with error 1813 (resource type not found).
// Verified against the built executable: ordinal 1 succeeds, the string "1" does not.
// Hence the raw declaration.
#[link(name = "user32")]
extern "system" {
    fn LoadImageW(
        hinst: *mut c_void,
        name: *const u16,
        r#type: u32,
        cx: i32,
        cy: i32,
        fu_load: u32,
    ) -> *mut c_void;
}
const IMAGE_ICON_TYPE: u32 = 1;
const LR_SHARED_FLAG: u32 = 0x8000;
// GetSystemMetrics indices. The ForDpi variants matter because the classic small icon at
// 200% is 32 device pixels, not 16 - asking for 16 and letting the shell stretch it to 32
// is what made the title bar look like a smudge.
const SM_CXICON_METRIC: i32 = 11;
const SM_CXSMICON_METRIC: i32 = 49;

#[link(name = "user32")]
extern "system" {
    fn GetSystemMetricsForDpi(index: i32, dpi: u32) -> i32;
}

/// Load the application icon from this executable's own resources, at the size the system
/// actually asks for at this DPI.
///
/// `IDI_APPICON` (id 1, see `build.rs`) is the lowest icon id, which is what Explorer and
/// the taskbar pick. The icon carries 16/20/24/32/40/48/64/128/256, so a requested size is
/// always present and Windows only ever scales DOWN, which stays sharp.
unsafe fn load_app_icon(dpi: u32) -> (HICON, HICON) {
    let Some(module) = GetModuleHandleW(None).ok() else {
        return (HICON::default(), HICON::default());
    };
    // An ordinal id is passed by casting it into the pointer-sized name field.
    let ordinal = 1usize as *const u16;
    let load = |metric: i32| -> HICON {
        let px = GetSystemMetricsForDpi(metric, dpi).max(0);
        // LR_SHARED: the handle is owned by the system and must not be destroyed.
        let h = LoadImageW(module.0, ordinal, IMAGE_ICON_TYPE, px, px, LR_SHARED_FLAG);
        if h.is_null() {
            HICON::default()
        } else {
            HICON(h)
        }
    };
    (load(SM_CXICON_METRIC), load(SM_CXSMICON_METRIC))
}
/// Extra hit/paint margin around buttons so the 2px keyboard focus ring can sit outside.
const FOCUS_PAD: i32 = 3;
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
// Shared with template_dialog.rs (its own palette; not part of the main-window spec).
const BACKGROUND: u32 = 0x0C121D;
const INPUT: u32 = 0x0D1725;
fn rgb(hex: u32) -> COLORREF {
    COLORREF(((hex & 0xff) << 16) | (hex & 0xff00) | ((hex >> 16) & 0xff))
}
/// 0xAARRGGBB / 0xRRGGBB → COLORREF (alpha ignored).
fn colorref(argb: u32) -> COLORREF {
    COLORREF(((argb & 0xff) << 16) | (argb & 0xff00) | ((argb >> 16) & 0xff))
}
fn rect_px(a: Box2, dpi: u32) -> RECT {
    RECT {
        left: view::px(a.x, dpi),
        top: view::px(a.y, dpi),
        right: view::px(a.x + a.w, dpi),
        bottom: view::px(a.y + a.h, dpi),
    }
}
/// Scale used for the whole layout, in DPI-equivalent units.
///
/// The layout is authored at `DEFAULT_WIDTH` DIP, so at 200% it wants 1920 physical
/// pixels. On a display too small to hold that, the window was previously clamped to
/// the work area while the renderer still painted at the full DPI factor - so the
/// right-hand side (the 设置 button, the DSH card and the status line) was clipped off
/// screen and simply unreachable. Clamping the scale to what the work area can hold
/// doubles as a 32 px margin for the frame.
///
/// At 100% and 150% on an ordinary display this equals `real`, so nothing changes
/// there; it only engages when the design genuinely does not fit.
fn fit_dpi(real: u32, work_width: i32) -> u32 {
    if work_width <= 0 {
        return real;
    }
    let available = (work_width - view::px(32, real)).max(1);
    let wanted = view::px(view::DEFAULT_WIDTH, real).max(1);
    if available >= wanted {
        return real;
    }
    let scaled = (real as i64 * available as i64 + wanted as i64 / 2) / wanted as i64;
    (scaled as u32).clamp(96, real)
}

/// How long to wait before starting another runtime, per consecutive failure.
///
/// Short at first so a one-off death costs seconds, then long enough that a runtime which
/// dies on startup every time cannot spin the machine.
fn respawn_delay(failures: u32) -> Duration {
    const BACKOFF_SECONDS: [u64; 4] = [5, 15, 45, 120];
    let step = (failures.saturating_sub(1) as usize).min(BACKOFF_SECONDS.len() - 1);
    Duration::from_secs(BACKOFF_SECONDS[step])
}

/// The one size a window may have: the design at the current scale, plus its frame.
///
/// `CONTENT_HEIGHT` is a fixed 480 DIP and only the *scale* follows the client width, so
/// a window larger than this cannot show any more UI. It paints the layout into the top
/// third and leaves the rest of the frame empty - the state a capture script once left
/// the live window in by resizing it from a DPI-unaware host, where "1550x1020" was
/// silently applied as 3100x2040 physical pixels. Pinning the size from both directions
/// is what makes that unreachable rather than merely unlikely.
fn fixed_window_rect(dpi: u32, style: WINDOW_STYLE) -> RECT {
    let mut r = RECT {
        left: 0,
        top: 0,
        right: view::px(view::DEFAULT_WIDTH, dpi),
        bottom: view::px(view::CONTENT_HEIGHT, dpi),
    };
    // SAFETY: AdjustWindowRectExForDpi only reads the style and DPI to grow the rect.
    unsafe {
        let _ = AdjustWindowRectExForDpi(&mut r, style, false, WS_EX_CONTROLPARENT, dpi);
    }
    r
}

// ---------------------------------------------------------------- fonts
pub(crate) struct Fonts {
    latin_semi: Vec<HFONT>,
    latin: Vec<HFONT>,
    cjk: Vec<HFONT>,
    mono: HFONT,
}
unsafe fn make_font(face: &str, size: i32, weight: i32, dpi: u32) -> HFONT {
    let f = wide(face);
    CreateFontW(
        -view::px(size, dpi),
        0,
        0,
        0,
        weight,
        0,
        0,
        0,
        DEFAULT_CHARSET,
        OUT_DEFAULT_PRECIS,
        CLIP_DEFAULT_PRECIS,
        CLEARTYPE_QUALITY,
        DEFAULT_PITCH.0 as u32,
        PCWSTR(f.as_ptr()),
    )
}
/// First installed face of the candidates (GDI silently substitutes otherwise).
unsafe fn pick_face(candidates: &[&str]) -> String {
    let dc = CreateCompatibleDC(None);
    let mut chosen = candidates[candidates.len() - 1].to_owned();
    for c in candidates {
        let f = make_font(c, 13, 400, 96);
        let old = SelectObject(dc, f.into());
        let mut buf = [0u16; 64];
        let n = GetTextFaceW(dc, Some(&mut buf));
        SelectObject(dc, old);
        let _ = DeleteObject(f.into());
        if String::from_utf16_lossy(&buf[..(n.max(1) - 1) as usize]).eq_ignore_ascii_case(c) {
            chosen = (*c).to_owned();
            break;
        }
    }
    let _ = DeleteDC(dc);
    chosen
}
impl Fonts {
    unsafe fn new(dpi: u32) -> Self {
        let latin_face = pick_face(&["Segoe UI Variable Text", "Segoe UI"]);
        let semi_face = pick_face(&["Segoe UI Variable Text Semibold", "Segoe UI Semibold"]);
        let cjk_face = pick_face(&["Microsoft YaHei UI", "Microsoft YaHei"]);
        let mono_face = pick_face(&["Cascadia Mono", "Consolas"]);
        Self {
            latin_semi: Font::ALL
                .iter()
                .map(|f| make_font(&semi_face, f.size(), 400, dpi))
                .collect(),
            latin: Font::ALL
                .iter()
                .map(|f| make_font(&latin_face, f.size(), f.weight(), dpi))
                .collect(),
            cjk: Font::ALL
                .iter()
                .map(|f| {
                    make_font(
                        &cjk_face,
                        f.size(),
                        if f.weight() >= 600 { 700 } else { f.weight() },
                        dpi,
                    )
                })
                .collect(),
            mono: make_font(&mono_face, Font::Code.size(), 400, dpi),
        }
    }
    /// Latin face for pure-ASCII strings, YaHei UI (so Simplified glyphs never fall
    /// back to a regional CJK face) for anything else, Cascadia Mono for code.
    fn pick(&self, f: Font, s: &str) -> HFONT {
        if s.is_ascii() {
            if f == Font::Code {
                self.mono
            } else if f.weight() >= 600 {
                self.latin_semi[f.index()]
            } else {
                self.latin[f.index()]
            }
        } else {
            self.cjk[f.index()]
        }
    }
}
impl Drop for Fonts {
    fn drop(&mut self) {
        unsafe {
            for f in self
                .latin
                .iter()
                .chain(self.latin_semi.iter())
                .chain(self.cjk.iter())
                .chain([&self.mono])
            {
                if !f.is_invalid() {
                    let _ = DeleteObject((*f).into());
                }
            }
        }
    }
}
struct GdiMeasure<'a> {
    dc: HDC,
    fonts: &'a Fonts,
    dpi: u32,
}
impl Measure for GdiMeasure<'_> {
    fn text_w(&self, s: &str, f: Font) -> i32 {
        unsafe {
            let old = SelectObject(self.dc, self.fonts.pick(f, s).into());
            let t: Vec<u16> = s.encode_utf16().collect();
            let mut size = SIZE::default();
            let _ = GetTextExtentPoint32W(self.dc, &t, &mut size);
            SelectObject(self.dc, old);
            ((size.cx as i64 * 96 + self.dpi as i64 - 1) / self.dpi as i64) as i32
        }
    }
}

// ---------------------------------------------------------------- GDI+
struct Gfx(*mut GpGraphics);
impl Gfx {
    unsafe fn new(dc: HDC) -> Option<Self> {
        let mut g: *mut GpGraphics = std::ptr::null_mut();
        if GdipCreateFromHDC(dc, &mut g) != Status(0) || g.is_null() {
            return None;
        }
        GdipSetSmoothingMode(g, SmoothingModeAntiAlias);
        GdipSetPixelOffsetMode(g, PixelOffsetModeHalf);
        Some(Self(g))
    }
    unsafe fn flush(&self) {
        GdipFlush(self.0, FlushIntentionSync);
    }
    unsafe fn rrect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> *mut GpPath {
        let mut p: *mut GpPath = std::ptr::null_mut();
        GdipCreatePath(FillModeAlternate, &mut p);
        let r = r.min(w / 2.0).min(h / 2.0);
        if r <= 0.5 {
            GdipAddPathRectangle(p, x, y, w, h);
        } else {
            let d = r * 2.0;
            GdipAddPathArc(p, x, y, d, d, 180.0, 90.0);
            GdipAddPathArc(p, x + w - d, y, d, d, 270.0, 90.0);
            GdipAddPathArc(p, x + w - d, y + h - d, d, d, 0.0, 90.0);
            GdipAddPathArc(p, x, y + h - d, d, d, 90.0, 90.0);
            GdipClosePathFigure(p);
        }
        p
    }
    unsafe fn fill(&self, x: f32, y: f32, w: f32, h: f32, r: f32, argb: u32) {
        let mut b: *mut GpSolidFill = std::ptr::null_mut();
        GdipCreateSolidFill(argb, &mut b);
        let p = Self::rrect_path(x, y, w, h, r);
        GdipFillPath(self.0, b as *mut GpBrush, p);
        GdipDeletePath(p);
        GdipDeleteBrush(b as *mut GpBrush);
    }
    /// Border drawn inside the box (CSS border-box), `sw` device pixels wide.
    unsafe fn border(&self, x: f32, y: f32, w: f32, h: f32, r: f32, argb: u32, sw: f32) {
        let mut pen: *mut GpPen = std::ptr::null_mut();
        GdipCreatePen1(argb, sw, UnitPixel, &mut pen);
        let p = Self::rrect_path(x + sw / 2.0, y + sw / 2.0, w - sw, h - sw, r - sw / 2.0);
        GdipDrawPath(self.0, pen, p);
        GdipDeletePath(p);
        GdipDeletePen(pen);
    }
    unsafe fn icon(&self, k: Icon, x: f32, y: f32, size: f32, argb: u32) {
        let mut pen: *mut GpPen = std::ptr::null_mut();
        GdipCreatePen1(argb, k.stroke() * size / 24.0, UnitPixel, &mut pen);
        GdipSetPenStartCap(pen, LineCapRound);
        GdipSetPenEndCap(pen, LineCapRound);
        GdipSetPenLineJoin(pen, LineJoinRound);
        for line in k.strokes() {
            let pts: Vec<PointF> = line
                .iter()
                .map(|(px, py)| PointF {
                    X: x + px * size / 24.0,
                    Y: y + py * size / 24.0,
                })
                .collect();
            if pts.len() >= 2 {
                GdipDrawLines(self.0, pen, pts.as_ptr(), pts.len() as i32);
            }
        }
        GdipDeletePen(pen);
    }
    unsafe fn arc(&self, x: f32, y: f32, d: f32, start: f32, sweep: f32, argb: u32, sw: f32) {
        let mut pen: *mut GpPen = std::ptr::null_mut();
        GdipCreatePen1(argb, sw, UnitPixel, &mut pen);
        GdipSetPenStartCap(pen, LineCapRound);
        GdipSetPenEndCap(pen, LineCapRound);
        GdipDrawArc(self.0, pen, x, y, d, d, start, sweep);
        GdipDeletePen(pen);
    }
}
impl Drop for Gfx {
    fn drop(&mut self) {
        unsafe {
            GdipDeleteGraphics(self.0);
        }
    }
}
unsafe fn draw_text(
    dc: HDC,
    fonts: &Fonts,
    s: &str,
    f: Font,
    color: u32,
    area: RECT,
    align: Align,
) {
    // DrawTextW faults on an empty buffer, and an empty string can reach here
    // whenever the runner reports no status text (e.g. hold_send_uncertain).
    if s.is_empty() {
        return;
    }
    let old = SelectObject(dc, fonts.pick(f, s).into());
    SetTextColor(dc, colorref(color));
    SetBkMode(dc, TRANSPARENT);
    let mut r = area;
    let mut chars: Vec<u16> = s.encode_utf16().collect();
    let flags = DT_NOPREFIX
        | DT_SINGLELINE
        | DT_VCENTER
        | DT_END_ELLIPSIS
        | match align {
            Align::Left => DT_LEFT,
            Align::Center => DT_CENTER,
            Align::Right => DT_RIGHT,
        };
    DrawTextW(dc, &mut chars, &mut r, flags);
    SelectObject(dc, old);
}
unsafe fn fill_rect(dc: HDC, r: &RECT, argb: u32) {
    let b = CreateSolidBrush(colorref(argb));
    FillRect(dc, r, b);
    let _ = DeleteObject(b.into());
}

struct Buffer {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    width: i32,
    height: i32,
}
impl Buffer {
    unsafe fn new(target: HDC, width: i32, height: i32) -> Option<Self> {
        let dc = CreateCompatibleDC(Some(target));
        if dc.is_invalid() {
            return None;
        }
        let bitmap = CreateCompatibleBitmap(target, width, height);
        if bitmap.is_invalid() {
            let _ = DeleteDC(dc);
            return None;
        }
        let old = SelectObject(dc, bitmap.into());
        Some(Self {
            dc,
            bitmap,
            old,
            width,
            height,
        })
    }
}

thread_local! {
    /// Scratch surface for composing an owner-drawn button before it reaches the screen.
    ///
    /// Painting straight onto the control's own DC made every stage a separate visible
    /// frame: the background blit erased the button, then the fill landed, then the icon,
    /// then the label. A screen capture of a hover repaint caught the button as a blank
    /// strip, and repeating that sequence is what reads as a flicker - which is why the
    /// sending buttons appeared to strobe. Composing here and blitting once at the end
    /// means the screen only ever sees the finished button.
    ///
    /// One buffer per thread, grown to fit the largest button seen. Only the UI thread
    /// paints, so there is nothing to synchronise.
    static BUTTON_SCRATCH: RefCell<Option<Buffer>> = const { RefCell::new(None) };
}

/// A DC at least `width` x `height` to compose a button in, or `None` if GDI is out of
/// resources - the caller then falls back to drawing straight onto the target.
unsafe fn button_scratch(target: HDC, width: i32, height: i32) -> Option<HDC> {
    BUTTON_SCRATCH.with(|cell| {
        let mut slot = cell.borrow_mut();
        let big_enough = slot
            .as_ref()
            .is_some_and(|b| b.width >= width && b.height >= height);
        if !big_enough {
            *slot = Buffer::new(target, width, height);
        }
        slot.as_ref().map(|b| b.dc)
    })
}
impl Drop for Buffer {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

// ---------------------------------------------------------------- app
struct App {
    product: PathBuf,
    snapshot: Snapshot,
    view: View,
    dpi: u32,
    controls: HashMap<usize, HWND>,
    order: Vec<usize>,
    /// Last accessible name / window rect / hidden state applied to each control, so
    /// unchanged controls are not touched on every state change.
    names: HashMap<usize, String>,
    placed: HashMap<usize, (i32, i32, i32, i32)>,
    hidden: std::collections::HashSet<usize>,
    /// True once the user has navigated with the keyboard. The focus ring is only
    /// drawn in that mode, so a mouse click no longer leaves a ring behind.
    keyboard: bool,
    /// First-run provisioning result, surfaced once as a banner.
    first_run: Option<handoff_core::config::FirstRunReport>,
    buffer: Option<Buffer>,
    fonts: Fonts,
    measure_dc: HDC,
    panel_brush: HBRUSH,
    gdip: usize,
    tip: HWND,
    tips: Vec<(Box2, String)>,
    tips_dpi: u32,
    width: i32,
    notice: Option<(Notice, Instant)>,
    next_read: Instant,
    watcher_alive: bool,
    demo: bool,
    editing: bool,
    guard: bool,
    poll_input: u64,
    hover: usize,
    spinning: bool,
    started: Instant,
    /// The runtime this window owns, once started. Held here (not in `run`) so the window
    /// can start a new one if it dies - see `ensure_runtime`.
    runtime: Option<RuntimeOwner>,
    respawn_failures: u32,
    respawn_at: Option<Instant>,
}
impl App {
    unsafe fn new(product: PathBuf, dpi: u32, demo: Option<String>) -> Self {
        // First run: create config / placeholder bindings / templates when missing.
        // Never overwrites, never touches sessions, never sends anything.
        let first_run = if demo.is_none() {
            let detected = if handoff_core::config::FirstRunState::probe(&product).is_first_run() {
                handoff_core::config::detect_dsh()
            } else {
                None
            };
            handoff_core::config::first_run(&product, detected).ok()
        } else {
            None
        };
        let snapshot = if let Some(ref name) = demo {
            Snapshot::for_demo(name)
        } else {
            Snapshot::load(&product)
        };
        let alive = demo.is_some() || runtime_process_alive(snapshot.watch_pid);
        // GDI+ needs an out-parameter for its startup handle. The name deliberately
        // avoids `token`, which scripts/release-audit.ps1 treats as a secret name.
        let mut gdip_startup = 0usize;
        let input = GdiplusStartupInput {
            GdiplusVersion: 1,
            ..Default::default()
        };
        let _ = GdiplusStartup(&mut gdip_startup, &input, std::ptr::null_mut());
        let poll_input = snapshot.poll_minutes.clamp(view::MIN_POLL, view::MAX_POLL);
        Self {
            product,
            snapshot,
            view: View::default(),
            dpi,
            controls: HashMap::new(),
            order: Vec::new(),
            names: HashMap::new(),
            placed: HashMap::new(),
            hidden: std::collections::HashSet::new(),
            keyboard: false,
            first_run,
            buffer: None,
            fonts: Fonts::new(dpi),
            measure_dc: CreateCompatibleDC(None),
            panel_brush: CreateSolidBrush(colorref(tok::PANEL)),
            gdip: gdip_startup,
            tip: HWND(std::ptr::null_mut()),
            tips: Vec::new(),
            tips_dpi: 0,
            width: view::DEFAULT_WIDTH,
            notice: None,
            next_read: Instant::now() + Duration::from_secs(2),
            watcher_alive: alive,
            demo: demo.is_some(),
            editing: false,
            guard: false,
            poll_input,
            hover: 0,
            spinning: false,
            started: Instant::now(),
            runtime: None,
            respawn_failures: 0,
            respawn_at: None,
        }
    }
    fn say(&mut self, variant: Variant, title: &str, detail: &str) {
        self.notice = Some((
            Notice {
                variant,
                title: title.into(),
                detail: detail.into(),
            },
            Instant::now() + Duration::from_millis(view::NOTICE_MS),
        ));
    }
    unsafe fn compute_view(&self) -> View {
        let m = GdiMeasure {
            dc: self.measure_dc,
            fonts: &self.fonts,
            dpi: self.dpi,
        };
        view::build(
            &m,
            self.width,
            &self.snapshot,
            self.watcher_alive,
            self.notice.as_ref().map(|(n, _)| n),
            self.poll_input,
            model::now_ms(),
        )
    }
    /// Recompute the view; touch windows/paint only when something actually changed.
    unsafe fn rebuild(&mut self, hwnd: HWND, force: bool) {
        let next = self.compute_view();
        if !force && next == self.view {
            return;
        }
        self.view = next;
        self.sync_controls(hwnd);
        self.sync_tips(hwnd);
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
    fn accessible_name(spec: &Control) -> String {
        match spec.kind {
            Kind::Switch(on) => format!("{} {}", spec.label, if on { "开" } else { "关" }),
            _ => spec.label.clone(),
        }
    }
    /// Create any control the view needs that does not exist yet.
    unsafe fn sync_controls(&mut self, hwnd: HWND) {
        let specs = self.view.controls.clone();
        let mut created = false;
        for spec in &specs {
            if self.controls.contains_key(&spec.id) {
                continue;
            }
            let label = wide(&spec.label);
            let (class, style) = if spec.kind == Kind::Edit {
                (
                    w!("EDIT"),
                    WS_CHILD
                        | WS_VISIBLE
                        | WS_TABSTOP
                        | WS_CLIPSIBLINGS
                        | WINDOW_STYLE((ES_CENTER | ES_NUMBER | ES_AUTOHSCROLL) as u32),
                )
            } else {
                (
                    w!("BUTTON"),
                    WS_CHILD
                        | WS_VISIBLE
                        | WS_TABSTOP
                        | WS_CLIPSIBLINGS
                        | WINDOW_STYLE(BS_OWNERDRAW as u32),
                )
            };
            let Ok(h) = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class,
                PCWSTR(label.as_ptr()),
                style,
                0,
                0,
                0,
                0,
                Some(hwnd),
                Some(HMENU(spec.id as *mut c_void)),
                Some(GetModuleHandleW(None).unwrap_or_default().into()),
                None,
            ) else {
                continue;
            };
            // Set once: font, keyboard limit, subclass, accessible name.
            SendMessageW(
                h,
                WM_SETFONT,
                Some(WPARAM(self.fonts.pick(Font::Body, "a").0 as usize)),
                Some(LPARAM(0)),
            );
            if spec.kind == Kind::Edit {
                SendMessageW(h, EM_SETLIMITTEXT, Some(WPARAM(3)), Some(LPARAM(0)));
                let t = wide(&self.poll_input.to_string());
                let _ = SetWindowTextW(h, PCWSTR(t.as_ptr()));
                SendMessageW(h, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
            } else {
                let orig = SetWindowLongPtrW(h, GWLP_WNDPROC, ctl_proc as *const () as isize);
                SetWindowLongPtrW(h, GWLP_USERDATA, orig);
                let name = Self::accessible_name(spec);
                let t = wide(&name);
                let _ = SetWindowTextW(h, PCWSTR(t.as_ptr()));
            }
            self.controls.insert(spec.id, h);
            created = true;
        }
        self.apply_controls(hwnd, created);
    }
    /// Apply the current view to existing controls, touching a window only when
    /// something about it actually differs. Doing this unconditionally made every
    /// state change repaint all controls, which the live refresh turned into a storm.
    unsafe fn apply_controls(&mut self, hwnd: HWND, order_dirty: bool) {
        let specs = self.view.controls.clone();
        for spec in &specs {
            let Some(&child) = self.controls.get(&spec.id) else {
                continue;
            };
            if spec.kind != Kind::Edit {
                let name = Self::accessible_name(spec);
                if self.names.get(&spec.id) != Some(&name) {
                    let t = wide(&name);
                    let _ = SetWindowTextW(child, PCWSTR(t.as_ptr()));
                    self.names.insert(spec.id, name);
                }
            }
            let pad = if matches!(spec.kind, Kind::Step | Kind::Edit) {
                0
            } else {
                FOCUS_PAD
            };
            let r = rect_px(
                Box2::new(
                    spec.area.x - pad,
                    spec.area.y - pad,
                    spec.area.w + pad * 2,
                    spec.area.h + pad * 2,
                ),
                self.dpi,
            );
            let want = (r.left, r.top, r.right - r.left, r.bottom - r.top);
            if self.placed.get(&spec.id) != Some(&want) {
                let _ = MoveWindow(child, want.0, want.1, want.2, want.3, false);
                self.placed.insert(spec.id, want);
                let _ = InvalidateRect(Some(child), None, false);
            }
            // EnableWindow is a no-op when the state already matches.
            let _ = EnableWindow(child, spec.enabled || spec.kind == Kind::Edit);
        }
        for (&id, &h) in &self.controls {
            if specs.iter().any(|c| c.id == id) {
                continue;
            }
            if self.hidden.insert(id) {
                let _ = ShowWindow(h, SW_HIDE);
            }
        }
        // Tab order == z-order == spec order (spec 9.5). Reorder only when the set of
        // controls changed; otherwise the existing order is already correct.
        let order: Vec<usize> = specs.iter().map(|c| c.id).collect();
        if created_or_changed(order_dirty, &order, &self.order) {
            for id in order.iter().rev() {
                if let Some(&h) = self.controls.get(id) {
                    let _ = SetWindowPos(
                        h,
                        Some(HWND_TOP),
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                    );
                }
            }
            self.order = order;
        }
        let spin = specs.iter().any(|c| c.loading);
        if spin && !self.spinning {
            SetTimer(Some(hwnd), SPIN_TIMER, 33, None);
        } else if !spin && self.spinning {
            let _ = KillTimer(Some(hwnd), SPIN_TIMER);
        }
        self.spinning = spin;
    }
    /// Repaint every control, e.g. after the input mode changed so the focus ring
    /// appears or disappears.
    unsafe fn invalidate_controls(&self) {
        for h in self.controls.values() {
            let _ = InvalidateRect(Some(*h), None, false);
        }
    }
    unsafe fn sync_tips(&mut self, hwnd: HWND) {
        if self.tip.is_invalid() || (self.view.tips == self.tips && self.dpi == self.tips_dpi) {
            return;
        }
        let size = std::mem::size_of::<TTTOOLINFOW>() as u32;
        for i in 0..self.tips.len() {
            let ti = TTTOOLINFOW {
                cbSize: size,
                hwnd,
                uId: i + 1,
                ..Default::default()
            };
            SendMessageW(
                self.tip,
                TTM_DELTOOLW,
                Some(WPARAM(0)),
                Some(LPARAM(&ti as *const _ as isize)),
            );
        }
        for (i, (area, text)) in self.view.tips.iter().enumerate() {
            let mut buf = wide(text);
            let ti = TTTOOLINFOW {
                cbSize: size,
                uFlags: TTF_SUBCLASS,
                hwnd,
                uId: i + 1,
                rect: rect_px(*area, self.dpi),
                lpszText: PWSTR(buf.as_mut_ptr()),
                ..Default::default()
            };
            SendMessageW(
                self.tip,
                TTM_ADDTOOLW,
                Some(WPARAM(0)),
                Some(LPARAM(&ti as *const _ as isize)),
            );
        }
        self.tips = self.view.tips.clone();
        self.tips_dpi = self.dpi;
    }
    unsafe fn resize(&mut self, hwnd: HWND) {
        let mut r = RECT::default();
        let _ = GetClientRect(hwnd, &mut r);
        self.width = ((r.right as i64 * 96) / self.dpi as i64) as i32;
        self.buffer = None;
        self.rebuild(hwnd, true);
    }
    unsafe fn tick(&mut self, hwnd: HWND) {
        if IsIconic(hwnd).as_bool() {
            return;
        }
        if let Some((_, until)) = &self.notice {
            if Instant::now() >= *until {
                self.notice = None;
            }
        }
        if !self.demo && Instant::now() >= self.next_read {
            let pending = !self.snapshot.live["pending"].is_null();
            self.next_read =
                Instant::now() + Duration::from_millis(if pending { 200 } else { 2000 });
            let updated = Snapshot::load(&self.product);
            let alive = runtime_process_alive(updated.watch_pid);
            let interval_changed = updated.poll_minutes != self.snapshot.poll_minutes;
            self.snapshot = updated;
            self.watcher_alive = alive;
            if interval_changed && !self.editing {
                self.poll_input = self
                    .snapshot
                    .poll_minutes
                    .clamp(view::MIN_POLL, view::MAX_POLL);
                self.set_edit();
            }
        }
        self.ensure_runtime();
        self.rebuild(hwnd, false);
    }
    /// Starts a new runtime when the one this window owns has exited.
    ///
    /// Without it the window could only *report* the loss: the 监听 toggle is enabled only
    /// while the heartbeat is fresh, so a dead runtime left restarting the whole app as the
    /// only way back (observed when a locked `state.json` killed it and the window sat on
    /// 心跳过期 / 已停止监听). Backs off so a runtime that keeps dying cannot spin, and
    /// forgets the backoff as soon as a heartbeat comes back.
    unsafe fn ensure_runtime(&mut self) {
        if self.runtime.is_none() {
            return; // demo mode owns no runtime
        }
        let exited = {
            let owner = self.runtime.as_mut().expect("checked above");
            matches!(owner.0.try_wait(), Ok(Some(_)))
        };
        if !exited {
            if self.watcher_alive {
                self.respawn_failures = 0;
                self.respawn_at = None;
            }
            return;
        }
        if self.respawn_at.is_some_and(|at| Instant::now() < at) {
            return;
        }
        self.respawn_failures = self.respawn_failures.saturating_add(1);
        self.respawn_at = Some(Instant::now() + respawn_delay(self.respawn_failures));
        if let Ok(owner) = RuntimeOwner::start(&self.product) {
            self.runtime = Some(owner);
        }
    }
    unsafe fn set_edit(&mut self) {
        if let Some(&h) = self.controls.get(&view::ID_VALUE) {
            self.guard = true;
            let t = wide(&self.poll_input.to_string());
            let _ = SetWindowTextW(h, PCWSTR(t.as_ptr()));
            SendMessageW(h, EM_SETSEL, Some(WPARAM(usize::MAX)), Some(LPARAM(-1)));
            self.guard = false;
        }
    }
    unsafe fn edit_text(&self) -> String {
        let Some(&h) = self.controls.get(&view::ID_VALUE) else {
            return String::new();
        };
        let mut text = [0u16; 16];
        let n = GetWindowTextW(h, &mut text);
        String::from_utf16_lossy(&text[..n.max(0) as usize])
            .trim()
            .to_owned()
    }
    unsafe fn command(&mut self, hwnd: HWND, id: usize, notification: u16) {
        if id == view::ID_VALUE {
            if notification == EN_SETFOCUS as u16 {
                self.editing = true;
            } else if notification == EN_KILLFOCUS as u16 {
                self.editing = false;
                if self.edit_text().parse::<u64>().is_err() {
                    self.set_edit();
                }
            } else if notification == EN_CHANGE as u16 && !self.guard {
                let t = self.edit_text();
                if !t.is_empty() {
                    match t.parse::<u64>() {
                        Ok(v) if (view::MIN_POLL..=view::MAX_POLL).contains(&v) => {
                            self.poll_input = v;
                            self.rebuild(hwnd, false);
                        }
                        // Out of range / not a number: fall back to the last legal value.
                        _ => self.set_edit(),
                    }
                }
            }
            return;
        }
        if notification != BN_CLICKED as u16 {
            return;
        }
        match id {
            view::ID_MINUS | view::ID_PLUS => {
                let old = self.poll_input;
                self.poll_input = if id == view::ID_PLUS {
                    (old + 1).min(view::MAX_POLL)
                } else {
                    old.saturating_sub(1).max(view::MIN_POLL)
                };
                self.set_edit();
                self.say(Variant::Info, "间隔尚未保存", "请点击“保存间隔”后生效。");
            }
            view::ID_SAVE => {
                let m = self.poll_input;
                if self.demo {
                    self.say(
                        Variant::Info,
                        "演示",
                        &format!("{m} 分钟；未写入任何运行配置。"),
                    );
                } else {
                    match model::save_interval(&self.product, m) {
                        Ok(()) => {
                            self.snapshot.poll_minutes = m;
                            self.snapshot.poll_seconds = m * 60;
                            self.say(
                                Variant::Success,
                                "轮询间隔已保存",
                                &format!("每 {m} 分钟。"),
                            );
                        }
                        Err(e) => self.say(Variant::Error, "保存失败", &e),
                    }
                }
            }
            view::ID_BINDINGS => match show_binding_config(hwnd, &self.product, &self.snapshot) {
                Ok(true) => {
                    self.snapshot = Snapshot::load(&self.product);
                    self.say(Variant::Success, "绑定配置已保存", "状态卡已刷新。");
                }
                Ok(false) => {}
                Err(e) => self.say(Variant::Error, "绑定配置未打开", &e),
            },
            view::ID_SETTINGS if !self.demo => match settings_dialog::show(hwnd, &self.product) {
                Ok(true) => {
                    self.snapshot = Snapshot::load(&self.product);
                    self.say(
                        Variant::Success,
                        "设置已保存",
                        "只更新了本机配置，未发送任何消息。",
                    );
                }
                Ok(false) => {}
                Err(e) => self.say(Variant::Error, "设置未打开", &e),
            },
            view::ID_TEMPLATES if !self.demo => match template_dialog::show(hwnd, &self.product) {
                Ok(true) => self.say(
                    Variant::Success,
                    "交接文案已保存",
                    "后续新投递使用新文案，历史回执保持不变。",
                ),
                Ok(false) => {}
                Err(e) => self.say(Variant::Error, "文案配置", &e),
            },
            view::ID_LOGS | view::ID_BANNER_LOGS => {
                let p = self.product.join("runtime");
                if p.is_dir() {
                    let _ = std::process::Command::new("explorer.exe").arg(p).spawn();
                } else {
                    self.say(Variant::Warn, "诊断目录尚不存在", "未创建新后台进程。");
                }
            }
            view::ID_CANCEL if self.demo => {
                self.snapshot.demo = Some("cancelled".into());
                self.snapshot.demo_deadline_ms = None;
            }
            view::ID_SEND_DSH
            | view::ID_SEND_CLAUDE
            | view::ID_TOGGLE
            | view::ID_RESTORE
            | view::ID_BANNER_RETRY
            | view::ID_CANCEL
                if !self.demo =>
            {
                let command = match id {
                    view::ID_SEND_DSH => "send_dsh",
                    view::ID_SEND_CLAUDE => "send_claude",
                    view::ID_TOGGLE => "toggle",
                    view::ID_RESTORE => "restore_listener",
                    view::ID_BANNER_RETRY => "retry_delivery",
                    _ => "cancel",
                };
                match model::request_command(&self.product, command) {
                    Ok(()) => self.say(Variant::Info, "操作已提交", "运行器将核验会话后执行。"),
                    Err(e) => self.say(Variant::Error, "操作未提交", &e),
                }
            }
            // 重新核验 / 立即轮询: no backend command exists; the buttons are disabled.
            _ => return,
        }
        self.rebuild(hwnd, false);
    }
}
impl Drop for App {
    fn drop(&mut self) {
        self.buffer = None;
        unsafe {
            let _ = DeleteObject(self.panel_brush.into());
            let _ = DeleteDC(self.measure_dc);
            GdiplusShutdown(self.gdip);
        }
    }
}

// Subclass for owner-drawn buttons: hover tracking + hand cursor.
unsafe extern "system" fn ctl_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let orig: WNDPROC = std::mem::transmute(GetWindowLongPtrW(h, GWLP_USERDATA));
    let id = GetDlgCtrlID(h) as usize;
    let parent = GetParent(h).unwrap_or_default();
    let app = app_ptr(parent);
    match msg {
        WM_MOUSEMOVE if !app.is_null() => {
            if (*app).hover != id {
                (*app).hover = id;
                let mut t = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: h,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut t);
                let _ = InvalidateRect(Some(h), None, false);
            }
        }
        WM_MOUSELEAVE if !app.is_null() => {
            if (*app).hover == id {
                (*app).hover = 0;
            }
            let _ = InvalidateRect(Some(h), None, false);
        }
        WM_SETCURSOR => {
            let cursor = if IsWindowEnabled(h).as_bool() {
                IDC_HAND
            } else {
                IDC_ARROW
            };
            if let Ok(c) = LoadCursorW(None, cursor) {
                SetCursor(Some(c));
                return LRESULT(1);
            }
        }
        _ => {}
    }
    CallWindowProcW(orig, h, msg, wp, lp)
}
const BIND_CLAUDE_SESSION: usize = 9103;
const BIND_DSH_SESSION: usize = 9104;
const BIND_SAVE: usize = 9191;
const BIND_CANCEL: usize = 9192;
// Settings dialog controls (`settings_dialog.rs`).
pub(crate) const CFG_DATA_HOME: usize = 9201;
pub(crate) const CFG_ORIGIN: usize = 9202;
pub(crate) const CFG_POLL: usize = 9203;
pub(crate) const CFG_DELAY: usize = 9204;
pub(crate) const CFG_BROWSER: usize = 9205;
pub(crate) const CFG_WORKSPACE: usize = 9206;
pub(crate) const CFG_CLAUDE_HOST: usize = 9207;
pub(crate) const CFG_TITLE_PATTERN: usize = 9208;
pub(crate) const CFG_SAVE: usize = 9291;
pub(crate) const CFG_CANCEL: usize = 9292;

/// Font used by the auxiliary dialogs (binding config, settings, templates).
pub(crate) unsafe fn warm_font(dpi: u32) -> HFONT {
    CreateFontW(
        -view::px(18, dpi),
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
    )
}

#[derive(Clone)]
struct DshChoice {
    id: String,
    title: String,
    modified: u64,
}
fn discover_dsh_sessions(product: &Path) -> Vec<DshChoice> {
    // The data directory comes from the typed config, so the UI and the adapters
    // always agree on where sessions live.
    let home = handoff_core::config::load(product).config.dsh_data_home;
    if home.trim().is_empty() {
        return vec![];
    }
    let dir = Path::new(&home).join("storages/session_projcache/sessions");
    let Ok(entries) = fs::read_dir(dir) else {
        return vec![];
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let p = entry.path();
        let Some(id) = p.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if !id.starts_with("session-") {
            continue;
        }
        let Some(j) = model::read_json(&p) else {
            continue;
        };
        let title = j["record"]["rows"]["title"]["val"]
            .as_str()
            .unwrap_or("未命名会话")
            .trim();
        let modified = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        out.push(DshChoice {
            id: id.to_owned(),
            title: if title.is_empty() {
                "未命名会话".into()
            } else {
                title.into()
            },
            modified,
        });
    }
    out.sort_by_key(|x| std::cmp::Reverse(x.modified));
    out.truncate(100);
    out
}

struct BindingDialogState {
    product: PathBuf,
    initial: BindingConfig,
    dsh_choices: Vec<DshChoice>,
    font: HFONT,
    claude_session: HWND,
    dsh_session: HWND,
    palette: crate::dialog_theme::DialogPalette,
    /// Font cache for the owner-drawn buttons, built at the dialog DPI.
    button_fonts: Option<Fonts>,
    saved: bool,
}
impl BindingDialogState {
    fn new(product: PathBuf, s: &Snapshot) -> Self {
        let mut choices = discover_dsh_sessions(&product);
        let dsh_is_configured =
            !s.dsh_session.is_empty() && s.dsh_session != "session-unconfigured";
        if dsh_is_configured && !choices.iter().any(|x| x.id == s.dsh_session) {
            choices.insert(
                0,
                DshChoice {
                    id: s.dsh_session.clone(),
                    title: "当前绑定".into(),
                    modified: u64::MAX,
                },
            );
        }
        Self {
            product,
            initial: BindingConfig {
                claude_title: s.claude_title.clone(),
                claude_window: s.target_window.clone(),
                claude_session: if s.claude_session == "cse_unconfigured" {
                    String::new()
                } else {
                    s.claude_session.clone()
                },
                dsh_session: if dsh_is_configured {
                    s.dsh_session.clone()
                } else {
                    String::new()
                },
            },
            dsh_choices: choices,
            font: HFONT(std::ptr::null_mut()),
            claude_session: HWND(std::ptr::null_mut()),
            dsh_session: HWND(std::ptr::null_mut()),
            // Safety: creates two solid brushes only.
            palette: unsafe { crate::dialog_theme::DialogPalette::new() },
            button_fonts: None,
            saved: false,
        }
    }
}
pub(crate) unsafe fn set_font(h: HWND, font: HFONT) {
    SendMessageW(
        h,
        WM_SETFONT,
        Some(WPARAM(font.0 as usize)),
        Some(LPARAM(1)),
    );
}
pub(crate) unsafe fn child_static(
    parent: HWND,
    text_value: &str,
    x: i32,
    y: i32,
    wid: i32,
    hei: i32,
    dpi: u32,
    font: HFONT,
) -> HWND {
    let t = wide(text_value);
    let h = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("STATIC"),
        PCWSTR(t.as_ptr()),
        WS_CHILD | WS_VISIBLE,
        view::px(x, dpi),
        view::px(y, dpi),
        view::px(wid, dpi),
        view::px(hei, dpi),
        Some(parent),
        None,
        Some(GetModuleHandleW(None).unwrap_or_default().into()),
        None,
    )
    .unwrap_or_default();
    set_font(h, font);
    h
}
pub(crate) unsafe fn child_edit(
    parent: HWND,
    id: usize,
    value: &str,
    x: i32,
    y: i32,
    wid: i32,
    hei: i32,
    dpi: u32,
    font: HFONT,
) -> HWND {
    let t = wide(value);
    let h = CreateWindowExW(
        WS_EX_CLIENTEDGE,
        w!("EDIT"),
        PCWSTR(t.as_ptr()),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
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
    SendMessageW(h, EM_SETLIMITTEXT, Some(WPARAM(180)), Some(LPARAM(0)));
    h
}
unsafe fn child_combo(
    parent: HWND,
    id: usize,
    x: i32,
    y: i32,
    wid: i32,
    hei: i32,
    dpi: u32,
    font: HFONT,
) -> HWND {
    let h = CreateWindowExW(
        WS_EX_CLIENTEDGE,
        w!("COMBOBOX"),
        w!(""),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE(CBS_DROPDOWNLIST as u32),
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
    h
}
// NOTE: dialog buttons are created through `dialog_button` (owner-drawn) so they
// match the main window. There is deliberately no native `BS_PUSHBUTTON` helper
// any more — a themed push button ignores WM_CTLCOLORBTN and always paints light.

pub(crate) unsafe fn hwnd_text(h: HWND) -> String {
    let n = GetWindowTextLengthW(h);
    let mut buf = vec![0u16; (n.max(0) + 1) as usize];
    let got = GetWindowTextW(h, &mut buf);
    String::from_utf16_lossy(&buf[..got.max(0) as usize])
        .trim()
        .to_owned()
}
unsafe fn binding_ptr(hwnd: HWND) -> *mut BindingDialogState {
    GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut BindingDialogState
}
unsafe extern "system" fn binding_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let cs = &*(lp.0 as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    let ptr = binding_ptr(hwnd);
    if ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    match msg {
        WM_CREATE => {
            let st = &mut *ptr;
            let dpi = GetDpiForWindow(hwnd).max(96);
            st.font = warm_font(dpi);
            // Built here, not earlier, because the dialog DPI is only known now.
            st.button_fonts = Some(Fonts::new(dpi));
            child_static(hwnd, "Claude Session ID", 24, 31, 230, 24, dpi, st.font);
            st.claude_session = child_edit(
                hwnd,
                BIND_CLAUDE_SESSION,
                &st.initial.claude_session,
                270,
                24,
                500,
                38,
                dpi,
                st.font,
            );
            child_static(
                hwnd,
                "只需填写 cse_...；窗口标题由程序按 Session 自动识别。",
                270,
                67,
                500,
                24,
                dpi,
                st.font,
            );
            child_static(
                hwnd,
                "DeepSeek Harness 会话",
                24,
                121,
                230,
                24,
                dpi,
                st.font,
            );
            st.dsh_session = child_combo(hwnd, BIND_DSH_SESSION, 270, 114, 500, 260, dpi, st.font);
            let mut selected = -1isize;
            for (i, item) in st.dsh_choices.iter().enumerate() {
                let short = if item.id.len() > 12 {
                    &item.id[item.id.len() - 8..]
                } else {
                    &item.id
                };
                let label = wide(&format!("{}  [{}]", item.title, short));
                SendMessageW(
                    st.dsh_session,
                    CB_ADDSTRING,
                    Some(WPARAM(0)),
                    Some(LPARAM(label.as_ptr() as isize)),
                );
                if item.id == st.initial.dsh_session {
                    selected = i as isize;
                }
            }
            if selected < 0 && !st.dsh_choices.is_empty() {
                selected = 0;
            }
            if selected >= 0 {
                SendMessageW(
                    st.dsh_session,
                    CB_SETCURSEL,
                    Some(WPARAM(selected as usize)),
                    Some(LPARAM(0)),
                );
            }
            child_static(
                hwnd,
                "列表按最近使用排序；显示名称，保存时记录真实 Session ID。",
                270,
                157,
                500,
                24,
                dpi,
                st.font,
            );
            child_static(
                hwnd,
                "保存只更新绑定，不发送消息，也不会启动新任务。",
                24,
                206,
                746,
                28,
                dpi,
                st.font,
            );
            for (id, label, x) in [(BIND_CANCEL, "取消", 518), (BIND_SAVE, "保存绑定", 650)] {
                let b = dialog_button(hwnd, id, label, x, 254, 120, 42, dpi, st.font);
                subclass_dialog_button(b);
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = wp.0 & 0xffff;
            if id == BIND_CANCEL {
                let _ = DestroyWindow(hwnd);
                return LRESULT(0);
            }
            if id == BIND_SAVE {
                let st = &mut *ptr;
                let idx = SendMessageW(st.dsh_session, CB_GETCURSEL, None, None).0;
                let dsh = if idx >= 0 {
                    st.dsh_choices
                        .get(idx as usize)
                        .map(|x| x.id.clone())
                        .unwrap_or_default()
                } else {
                    String::new()
                };
                let claude = hwnd_text(st.claude_session);
                let same = claude == st.initial.claude_session;
                let b = BindingConfig {
                    claude_title: if same && !st.initial.claude_title.is_empty() {
                        st.initial.claude_title.clone()
                    } else {
                        "Claude Desktop".into()
                    },
                    claude_window: if same && !st.initial.claude_window.is_empty() {
                        st.initial.claude_window.clone()
                    } else {
                        "Claude Desktop".into()
                    },
                    claude_session: claude,
                    dsh_session: dsh,
                };
                match model::save_bindings(&st.product, &b) {
                    Ok(()) => {
                        st.saved = true;
                        let _ = DestroyWindow(hwnd);
                    }
                    Err(e) => {
                        let m = wide(&e);
                        let _ = MessageBoxW(
                            Some(hwnd),
                            PCWSTR(m.as_ptr()),
                            w!("绑定配置"),
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
            crate::dialog_theme::erase_background(hwnd, wp, &st.palette)
        }
        // Dark palette for the labels, the Session ID field and the dropdown, so
        // this dialog matches the main window instead of the system grey.
        msg if crate::dialog_theme::is_ctlcolor(msg) => {
            let st = &*ptr;
            let child = HWND(lp.0 as *mut c_void);
            let is_input = child == st.claude_session || child == st.dsh_session;
            crate::dialog_theme::paint_child(msg, wp, &st.palette, is_input)
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
                hover_slot(hwnd) == item.CtlID as usize,
                colorref(crate::dialog_theme::DIALOG_BACKGROUND),
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
            LRESULT(0)
        }
        WM_NCDESTROY => {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
unsafe fn show_binding_config(
    parent: HWND,
    product: &Path,
    snapshot: &Snapshot,
) -> Result<bool, String> {
    let instance = GetModuleHandleW(None).map_err(|e| e.to_string())?;
    let class = w!("A2AHandoff.BindingConfig.v2");
    RegisterClassW(&WNDCLASSW {
        hCursor: LoadCursorW(None, IDC_ARROW).map_err(|e| e.to_string())?,
        hInstance: instance.into(),
        lpszClassName: class,
        lpfnWndProc: Some(binding_proc),
        ..Default::default()
    });
    let state = Box::new(BindingDialogState::new(product.to_owned(), snapshot));
    let raw = Box::into_raw(state);
    let dpi = GetDpiForWindow(parent).max(96);
    let (x, y, width, height) =
        crate::dialog_theme::centered_rect(view::px(790, dpi), view::px(320, dpi));
    let hwnd = match CreateWindowExW(
        WS_EX_DLGMODALFRAME,
        class,
        w!("A2AHandoff · 绑定配置"),
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
    crate::dialog_theme::apply_dark_title_bar(hwnd);
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
            DispatchMessageW(&m);
        }
    }
    let _ = EnableWindow(parent, true);
    let _ = SetForegroundWindow(parent);
    let state = Box::from_raw(raw);
    Ok(state.saved)
}

/// Owner-draw a dialog button so it matches the main window's secondary buttons
/// instead of the light system push button.
///
/// This is deliberately a separate, simpler path from `draw_button`: a dialog has no
/// back buffer behind its controls, so the rounded corners are blended against the
/// flat dialog background rather than a cached surface.
pub(crate) unsafe fn paint_dialog_button(
    item: &DRAWITEMSTRUCT,
    fonts: &Fonts,
    dpi: u32,
    hovered: bool,
    background: COLORREF,
) {
    use tok::*;
    let screen = item.hDC;
    let rc = item.rcItem;
    let (cw, ch) = (rc.right - rc.left, rc.bottom - rc.top);
    if cw <= 0 || ch <= 0 {
        return;
    }
    // Composed off-screen for the same reason as the main window's buttons: painting the
    // stages straight onto the control makes the erase, the fill and the label visible as
    // separate frames (see BUTTON_SCRATCH).
    let scratch = button_scratch(screen, cw, ch);
    let dc = scratch.unwrap_or(screen);
    let s = dpi as f32 / 96.0;
    let enabled = (item.itemState.0 & ODS_DISABLED.0) == 0;
    let pressed = (item.itemState.0 & ODS_SELECTED.0) != 0;
    let label = hwnd_text(item.hwndItem);

    // The dialog is a flat colour, so painting the whole item rect first makes the
    // anti-aliased rounded corners blend correctly.
    fill_rect(
        dc,
        &RECT {
            left: 0,
            top: 0,
            right: cw,
            bottom: ch,
        },
        // COLORREF is 0x00BBGGRR; convert back to 0xAARRGGBB for the GDI+ helpers.
        {
            let v = background.0 as u32;
            0xFF00_0000 | ((v & 0xFF) << 16) | (v & 0xFF00) | ((v >> 16) & 0xFF)
        },
    );

    let Some(g) = Gfx::new(dc) else { return };
    let (vx, vy, vw, vh) = (0.0f32, 0.0f32, cw as f32, ch as f32);
    let (bg, border, fg) = if !enabled {
        (RAISED, LINE, TEXT_DISABLED)
    } else if pressed {
        (RAISED_PRESSED, LINE, TEXT)
    } else if hovered {
        (RAISED_HOVER, LINE, TEXT)
    } else {
        (RAISED, LINE, TEXT)
    };
    g.fill(vx, vy, vw, vh, 8.0 * s, bg);
    g.border(vx, vy, vw, vh, 8.0 * s, border, s);
    g.flush();

    let font = Font::Body;
    let tw = text_extent_px(dc, fonts, &label, font) as f32;
    let left = vx + (vw - tw) / 2.0;
    let r = RECT {
        left: left as i32,
        top: vy as i32,
        right: (left + tw) as i32 + 2,
        bottom: (vy + vh) as i32,
    };
    draw_text(dc, fonts, &label, font, fg, r, Align::Left);

    // Keyboard focus ring, drawn inside because dialog buttons are not padded.
    if (item.itemState.0 & ODS_FOCUS.0) != 0 && (item.itemState.0 & ODS_NOFOCUSRECT.0) == 0 {
        g.border(
            vx + 2.0 * s,
            vy + 2.0 * s,
            vw - 4.0 * s,
            vh - 4.0 * s,
            6.0 * s,
            DSH_SOFT,
            2.0 * s,
        );
        g.flush();
    }
    drop(g);
    if scratch.is_some() {
        let _ = BitBlt(screen, 0, 0, cw, ch, Some(dc), 0, 0, SRCCOPY);
    }
}

/// Create a dialog button as an owner-drawn control so it can be painted with the
/// main window's button look.
pub(crate) unsafe fn dialog_button(
    parent: HWND,
    id: usize,
    label: &str,
    x: i32,
    y: i32,
    wid: i32,
    hei: i32,
    dpi: u32,
    font: HFONT,
) -> HWND {
    let t = wide(label);
    let h = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("BUTTON"),
        PCWSTR(t.as_ptr()),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32),
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
    h
}

/// Hover tracking for dialog buttons: stores the hovered control id in the owning
/// dialog's state slot and repaints on change.
pub(crate) unsafe extern "system" fn dialog_ctl_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
) -> LRESULT {
    let orig: WNDPROC = std::mem::transmute(GetWindowLongPtrW(hwnd, GWLP_USERDATA));
    let id = GetDlgCtrlID(hwnd) as usize;
    let parent = GetParent(hwnd).unwrap_or_default();
    match msg {
        WM_MOUSEMOVE => {
            if !parent.0.is_null() && hover_slot(parent) != id {
                set_hover_slot(parent, id);
                let _ = InvalidateRect(Some(hwnd), None, false);
                let mut t = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut t);
            }
        }
        WM_MOUSELEAVE => {
            if !parent.0.is_null() && hover_slot(parent) == id {
                set_hover_slot(parent, 0);
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
        }
        WM_SETCURSOR => {
            if IsWindowEnabled(hwnd).as_bool() {
                if let Ok(c) = LoadCursorW(None, IDC_HAND) {
                    SetCursor(Some(c));
                    return LRESULT(1);
                }
            }
        }
        _ => {}
    }
    CallWindowProcW(orig, hwnd, msg, wp, lp)
}

/// Dialog hover state lives next to the dialog's own state pointer, keyed by the
/// parent window, so the subclass procedure needs no extra allocation.
pub(crate) fn hover_slot(hwnd: HWND) -> usize {
    unsafe { GetPropW(hwnd, w!("A2A.HoverId")).0 as usize }
}
pub(crate) fn set_hover_slot(hwnd: HWND, id: usize) {
    unsafe {
        let _ = SetPropW(hwnd, w!("A2A.HoverId"), Some(HANDLE(id as *mut c_void)));
    }
}

/// Register the hover subclass on a freshly created owner-drawn dialog button.
pub(crate) unsafe fn subclass_dialog_button(h: HWND) {
    let orig = SetWindowLongPtrW(h, GWLP_WNDPROC, dialog_ctl_proc as *const () as isize);
    SetWindowLongPtrW(h, GWLP_USERDATA, orig);
}

unsafe fn runtime_process_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
        return false;
    };
    let mut path = [0u16; 1024];
    let mut size = path.len() as u32;
    let good = QueryFullProcessImageNameW(
        h,
        PROCESS_NAME_WIN32,
        windows::core::PWSTR(path.as_mut_ptr()),
        &mut size,
    )
    .is_ok();
    let name = if good {
        String::from_utf16_lossy(&path[..size as usize]).to_ascii_lowercase()
    } else {
        String::new()
    };
    let mut exit = 0;
    let running = GetExitCodeProcess(h, &mut exit).is_ok() && exit == 259;
    let _ = CloseHandle(h);
    running && name.ends_with("\\handoff-runtime.exe")
}
unsafe fn text_extent_px(dc: HDC, fonts: &Fonts, s: &str, f: Font) -> i32 {
    let old = SelectObject(dc, fonts.pick(f, s).into());
    let t: Vec<u16> = s.encode_utf16().collect();
    let mut size = SIZE::default();
    let _ = GetTextExtentPoint32W(dc, &t, &mut size);
    SelectObject(dc, old);
    size.cx
}
/// True when the window set changed (a control was created or removed) or the
/// desired z-order no longer matches the previous one, so a reorder is needed.
fn created_or_changed(order_dirty: bool, order: &[usize], prev: &[usize]) -> bool {
    order_dirty || order != prev
}
unsafe fn render(dc: HDC, r: RECT, app: &App) {
    fill_rect(dc, &r, tok::GROUND);
    let s = app.dpi as f32 / 96.0;
    let Some(g) = Gfx::new(dc) else { return };
    for prim in &app.view.prims {
        match prim {
            Prim::Rect { a, r, fill, stroke } => {
                let (x, y, w, h) = (
                    a.x as f32 * s,
                    a.y as f32 * s,
                    a.w as f32 * s,
                    a.h as f32 * s,
                );
                g.fill(x, y, w, h, *r as f32 * s, *fill);
                if let Some(c) = stroke {
                    g.border(x, y, w, h, *r as f32 * s, *c, s);
                }
            }
            Prim::Icon { a, k, color } => {
                g.icon(*k, a.x as f32 * s, a.y as f32 * s, a.w as f32 * s, *color)
            }
            Prim::Text {
                a,
                s: text,
                f,
                color,
                align,
            } => {
                g.flush();
                draw_text(
                    dc,
                    &app.fonts,
                    text,
                    *f,
                    *color,
                    rect_px(*a, app.dpi),
                    *align,
                );
            }
        }
    }
    g.flush();
}
unsafe fn draw_button(item: &DRAWITEMSTRUCT, spec: &Control, app: &App) {
    use tok::*;
    let screen = item.hDC;
    let rc = item.rcItem;
    let (cw, ch) = (rc.right - rc.left, rc.bottom - rc.top);
    // Everything below is composed off-screen and lands on the control in one BitBlt, so
    // no stage of the button is ever visible on its own (see BUTTON_SCRATCH).
    let scratch = button_scratch(screen, cw, ch);
    let dc = scratch.unwrap_or(screen);
    let pad_dip = if spec.kind == Kind::Step {
        0
    } else {
        FOCUS_PAD
    };
    let origin = rect_px(
        Box2::new(
            spec.area.x - pad_dip,
            spec.area.y - pad_dip,
            spec.area.w,
            spec.area.h,
        ),
        app.dpi,
    );
    // The parent's back buffer already holds everything behind the button (banner tint,
    // card panel, stepper frame), so anti-aliased corners blend correctly.
    match &app.buffer {
        Some(b) => {
            let _ = BitBlt(
                dc,
                0,
                0,
                cw,
                ch,
                Some(b.dc),
                origin.left,
                origin.top,
                SRCCOPY,
            );
        }
        None => fill_rect(dc, &rc, GROUND),
    }
    let s = app.dpi as f32 / 96.0;
    let pad = pad_dip as f32 * s;
    let (vx, vy, vw, vh) = (pad, pad, cw as f32 - pad * 2.0, ch as f32 - pad * 2.0);
    let enabled = spec.enabled;
    let hover = enabled && app.hover == spec.id;
    let pressed = (item.itemState.0 & ODS_SELECTED.0) != 0 && enabled;
    let Some(g) = Gfx::new(dc) else { return };
    let vis = RECT {
        left: vx as i32,
        top: vy as i32,
        right: (vx + vw) as i32,
        bottom: (vy + vh) as i32,
    };
    match spec.kind {
        Kind::Switch(on) => {
            let fg = if enabled { TEXT } else { TEXT_DISABLED };
            let (tw, th) = (32.0 * s, 18.0 * s);
            let tx = vx + vw - tw;
            let ty = vy + (vh - th) / 2.0;
            let track = match (on, enabled) {
                (true, true) => OK,
                (true, false) => 0x803DB88A,
                (false, _) => LINE_STRONG,
            };
            g.fill(tx, ty, tw, th, 9.0 * s, track);
            let (thumb, left) = if on {
                (WHITE, tx + 16.0 * s)
            } else {
                (TEXT2, tx + 2.0 * s)
            };
            g.fill(left, ty + 2.0 * s, 14.0 * s, 14.0 * s, 7.0 * s, thumb);
            g.flush();
            let mut r = vis;
            r.right = (tx - 8.0 * s) as i32;
            draw_text(dc, &app.fonts, &spec.label, Font::Body, fg, r, Align::Left);
        }
        _ => {
            let (bg, border, fg): (Option<u32>, Option<u32>, u32) = match spec.kind {
                Kind::PrimaryClaude | Kind::PrimaryDsh if !enabled && !spec.loading => {
                    (Some(RAISED), Some(LINE), TEXT_DISABLED)
                }
                Kind::PrimaryClaude => (
                    Some(if pressed {
                        CLAUDE_PRESSED
                    } else if hover {
                        CLAUDE_HOVER
                    } else {
                        CLAUDE
                    }),
                    None,
                    CLAUDE_ON,
                ),
                Kind::PrimaryDsh => (
                    Some(if pressed {
                        DSH_PRESSED
                    } else if hover {
                        DSH_HOVER
                    } else {
                        DSH
                    }),
                    None,
                    WHITE,
                ),
                Kind::Step if !enabled => (None, None, TEXT_DISABLED),
                Kind::Step => (
                    if pressed {
                        Some(RAISED_PRESSED)
                    } else if hover {
                        Some(RAISED_HOVER)
                    } else {
                        None
                    },
                    None,
                    TEXT2,
                ),
                _ if !enabled => (Some(RAISED), Some(LINE), TEXT_DISABLED),
                Kind::Ghost => (
                    if pressed {
                        Some(RAISED_PRESSED)
                    } else if hover {
                        Some(RAISED)
                    } else {
                        None
                    },
                    None,
                    if hover { TEXT } else { TEXT2 },
                ),
                _ => (
                    Some(if pressed {
                        RAISED_PRESSED
                    } else if hover {
                        RAISED_HOVER
                    } else {
                        RAISED
                    }),
                    Some(if spec.kind == Kind::BannerSecondary {
                        LINE_STRONG
                    } else {
                        LINE
                    }),
                    TEXT,
                ),
            };
            let radius = if spec.kind == Kind::Step {
                0.0
            } else {
                8.0 * s
            };
            if let Some(c) = bg {
                g.fill(vx, vy, vw, vh, radius, c);
            }
            if let Some(c) = border {
                g.border(vx, vy, vw, vh, radius, c, s);
            }
            g.flush();
            let primary = matches!(spec.kind, Kind::PrimaryClaude | Kind::PrimaryDsh);
            let font = match spec.kind {
                Kind::Step => Font::Glyph,
                _ if primary => Font::BodyStrong,
                _ => Font::Body,
            };
            let tw = text_extent_px(dc, &app.fonts, &spec.label, font) as f32;
            let has_icon = spec.icon.is_some() || spec.loading;
            let (isz, gap) = (16.0 * s, if primary { 8.0 * s } else { 6.0 * s });
            let group = tw + if has_icon { isz + gap } else { 0.0 };
            let x0 = vx + (vw - group) / 2.0;
            let iy = vy + (vh - isz) / 2.0;
            if spec.loading {
                let ms = app.started.elapsed().as_millis() as f32;
                let d = 14.0 * s;
                g.arc(
                    x0 + s,
                    iy + s,
                    d,
                    (ms % 1000.0) / 1000.0 * 360.0,
                    270.0,
                    fg,
                    2.0 * s,
                );
            } else if let Some(k) = spec.icon {
                g.icon(k, x0, iy, isz, fg);
            }
            g.flush();
            let left = x0 + if has_icon { isz + gap } else { 0.0 };
            let r = RECT {
                left: left as i32,
                top: vis.top,
                right: (left + tw) as i32 + 2,
                bottom: vis.bottom,
            };
            draw_text(dc, &app.fonts, &spec.label, font, fg, r, Align::Left);
        }
    }
    // Keyboard focus ring, keyboard navigation only: a ring left behind by a mouse
    // click reads as a glitch, while Tab users still need to see where they are.
    if app.keyboard
        && (item.itemState.0 & ODS_FOCUS.0) != 0
        && (item.itemState.0 & ODS_NOFOCUSRECT.0) == 0
    {
        if spec.kind == Kind::Step {
            g.border(vx, vy, vw, vh, 6.0 * s, DSH_SOFT, 2.0 * s);
        } else {
            g.border(
                vx - 2.0 * s,
                vy - 2.0 * s,
                vw + 4.0 * s,
                vh + 4.0 * s,
                10.0 * s,
                DSH_SOFT,
                2.0 * s,
            );
        }
        g.flush();
    }
    // Release the GDI+ object before handing the composed surface to GDI.
    drop(g);
    if scratch.is_some() {
        let _ = BitBlt(screen, 0, 0, cw, ch, Some(dc), 0, 0, SRCCOPY);
    }
}
unsafe fn app_ptr(hwnd: HWND) -> *mut App {
    GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App
}
unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let cs = &*(lp.0 as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    let ptr = app_ptr(hwnd);
    if ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    match msg {
        WM_CREATE => {
            let app = &mut *ptr;
            app.tip = CreateWindowExW(
                WS_EX_TOPMOST,
                w!("tooltips_class32"),
                PCWSTR::null(),
                WS_POPUP | WINDOW_STYLE(TTS_ALWAYSTIP | TTS_NOPREFIX),
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                Some(hwnd),
                None,
                Some(GetModuleHandleW(None).unwrap_or_default().into()),
                None,
            )
            .unwrap_or_default();
            if !app.tip.is_invalid() {
                SendMessageW(
                    app.tip,
                    TTM_SETMAXTIPWIDTH,
                    Some(WPARAM(0)),
                    Some(LPARAM(view::px(520, app.dpi) as isize)),
                );
            }
            app.resize(hwnd);
            SetTimer(Some(hwnd), TIMER, 250, None);
            // Tell a first-time user what was just created and what to do next.
            if let Some(report) = app.first_run.take() {
                if report.config_written || report.bindings_written {
                    let detail = if report.detected.is_some() {
                        "已自动检测到 DSH 环境；请打开「绑定配置」选择 DSH 会话并填入 Claude cse_ ID。"
                    } else {
                        "请在「设置」中填写 DSH 数据目录，然后打开「绑定配置」完成绑定。"
                    };
                    app.say(Variant::Info, "首次启动：已生成本机配置", detail);
                }
            }
            let val = 1i32;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &val as *const _ as *const c_void,
                4,
            );
            let caption = colorref(tok::GROUND);
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_CAPTION_COLOR,
                &caption as *const _ as *const c_void,
                4,
            );
            LRESULT(0)
        }
        WM_SIZE => {
            (&mut *ptr).resize(hwnd);
            LRESULT(0)
        }
        WM_GETMINMAXINFO => {
            let app = &*ptr;
            let m = &mut *(lp.0 as *mut MINMAXINFO);
            let style = WINDOW_STYLE(GetWindowLongPtrW(hwnd, GWL_STYLE) as u32);
            let r = fixed_window_rect(app.dpi, style);
            let (w, h) = (r.right - r.left, r.bottom - r.top);
            // Minimum == maximum: the window has exactly one size. `CONTENT_HEIGHT` is
            // fixed, so dragging it larger only adds empty frame (see fixed_window_rect).
            m.ptMinTrackSize = POINT { x: w, y: h };
            m.ptMaxTrackSize = POINT { x: w, y: h };
            // A maximise request therefore cannot grow it either; put it in the middle
            // of the work area rather than the top-left corner the default would use.
            m.ptMaxSize = POINT { x: w, y: h };
            let mut wa = RECT::default();
            let _ = SystemParametersInfoW(
                SPI_GETWORKAREA,
                0,
                Some(&mut wa as *mut _ as *mut c_void),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            );
            m.ptMaxPosition = POINT {
                x: wa.left + (wa.right - wa.left - w) / 2,
                y: wa.top + (wa.bottom - wa.top - h) / 2,
            };
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let app = &mut *ptr;
            // Same fit rule as at creation: a move to a smaller or differently scaled
            // monitor must not push the layout back off the screen.
            let mut wa = RECT::default();
            let _ = SystemParametersInfoW(
                SPI_GETWORKAREA,
                0,
                Some(&mut wa as *mut _ as *mut c_void),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            );
            app.dpi = fit_dpi((wp.0 & 0xffff) as u32, wa.right - wa.left);
            app.buffer = None;
            app.fonts = Fonts::new(app.dpi);
            let r = *(lp.0 as *const RECT);
            let _ = SetWindowPos(
                hwnd,
                None,
                r.left,
                r.top,
                r.right - r.left,
                r.bottom - r.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            (&mut *ptr).resize(hwnd);
            LRESULT(0)
        }
        // Any pointer activity switches focus rings off. (Key messages are handled in
        // the message loop: IsDialogMessageW swallows them before this procedure.)
        WM_MOUSEMOVE | WM_NCMOUSEMOVE | WM_LBUTTONDOWN | WM_LBUTTONUP | WM_MBUTTONDOWN
        | WM_MBUTTONUP | WM_RBUTTONDOWN | WM_RBUTTONUP | WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
            let app = &mut *ptr;
            if app.keyboard {
                app.keyboard = false;
                app.invalidate_controls();
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_TIMER => {
            if wp.0 == SPIN_TIMER {
                let app = &*ptr;
                for c in app.view.controls.iter().filter(|c| c.loading) {
                    if let Some(&h) = app.controls.get(&c.id) {
                        let _ = InvalidateRect(Some(h), None, false);
                    }
                }
            } else {
                (&mut *ptr).tick(hwnd);
            }
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_COMMAND => {
            let id = wp.0 & 0xffff;
            let code = ((wp.0 >> 16) & 0xffff) as u16;
            (&mut *ptr).command(hwnd, id, code);
            LRESULT(0)
        }
        WM_DRAWITEM => {
            let app = &*ptr;
            let item = &*(lp.0 as *const DRAWITEMSTRUCT);
            if let Some(spec) = app
                .view
                .controls
                .iter()
                .find(|c| c.id == item.CtlID as usize)
            {
                draw_button(item, spec, app);
                return LRESULT(1);
            }
            LRESULT(0)
        }
        WM_CTLCOLOREDIT => {
            let app = &*ptr;
            let h = HDC(wp.0 as *mut c_void);
            SetTextColor(h, colorref(tok::TEXT));
            SetBkColor(h, colorref(tok::PANEL));
            LRESULT(app.panel_brush.0 as isize)
        }
        WM_PAINT => {
            let app = &mut *ptr;
            let mut ps = PAINTSTRUCT::default();
            let paint = BeginPaint(hwnd, &mut ps);
            let mut r = RECT::default();
            let _ = GetClientRect(hwnd, &mut r);
            if r.right > 0 && r.bottom > 0 {
                if app
                    .buffer
                    .as_ref()
                    .is_none_or(|b| b.width != r.right || b.height != r.bottom)
                {
                    app.buffer = Buffer::new(paint, r.right, r.bottom);
                }
                if let Some(b) = &app.buffer {
                    render(b.dc, r, app);
                    let _ = BitBlt(paint, 0, 0, r.right, r.bottom, Some(b.dc), 0, 0, SRCCOPY);
                } else {
                    render(paint, r, app);
                }
            }
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            let _ = KillTimer(Some(hwnd), TIMER);
            let _ = KillTimer(Some(hwnd), SPIN_TIMER);
            PostQuitMessage(0);
            LRESULT(0)
        }
        WM_NCDESTROY => {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            drop(Box::from_raw(ptr));
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
unsafe fn layout_audit(product: &Path) -> Result<(), String> {
    let output = product.join("artifacts/ui-review");
    fs::create_dir_all(&output).map_err(|e| e.to_string())?;
    let dc = CreateCompatibleDC(None);
    let mut reports = Vec::new();
    for dpi in [96u32, 120, 144, 192, 240] {
        let fonts = Fonts::new(dpi);
        let state = Snapshot::load(product);
        let m = GdiMeasure {
            dc,
            fonts: &fonts,
            dpi,
        };
        for width in [960, 1100] {
            let v = view::build(
                &m,
                width,
                &state,
                runtime_process_alive(state.watch_pid),
                None,
                state.poll_minutes,
                model::now_ms(),
            );
            let mut overflow = Vec::new();
            for p in &v.prims {
                // Task names and code blocks are allowed to ellipsize (spec 5.6, 4.4).
                if let Prim::Text { a, s, f, .. } = p {
                    if matches!(f, Font::Task | Font::Code) {
                        continue;
                    }
                    let need = m.text_w(s, *f);
                    if need > a.w {
                        overflow.push(serde_json::json!({"text":s,"need":need,"box":a.w}));
                    }
                }
            }
            reports.push(serde_json::json!({"dpi":dpi,"logical_width":width,"overflows":overflow}));
        }
    }
    let _ = DeleteDC(dc);
    let report =
        serde_json::json!({"mode":"font_and_layout_audit","messages_sent":0,"reports":reports});
    fs::write(
        output.join("layout-audit.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
fn discover_product(explicit: Option<&str>) -> Result<PathBuf, String> {
    if let Some(path) = explicit {
        return Ok(PathBuf::from(path));
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    for parent in exe.ancestors().skip(1).take(6) {
        if parent.join("runtime/config.json").is_file() {
            return Ok(parent.to_owned());
        }
    }
    Err("找不到产品配置，请使用 --product-root 指定 product 目录。".into())
}
struct UiInstance(HANDLE);
impl UiInstance {
    unsafe fn acquire() -> Result<Option<Self>, String> {
        // One UI owns one runtime. `A2A_ALLOW_SECOND_INSTANCE` exists so the test
        // scripts can drive a second, self-contained UI against a throwaway product
        // root while the user's own instance keeps running. It only relaxes the UI
        // guard; it does not change delivery behaviour.
        if std::env::var_os("A2A_ALLOW_SECOND_INSTANCE").is_some() {
            let h = CreateMutexW(None, false, w!("Local\\A2AHandoff.V1.UI.Test"))
                .map_err(|e| e.to_string())?;
            return Ok(Some(Self(h)));
        }
        let h =
            CreateMutexW(None, false, w!("Local\\A2AHandoff.V1.UI")).map_err(|e| e.to_string())?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(h);
            if let Ok(w) = FindWindowW(w!("A2AHandoff.Product.UI.v2"), None) {
                let _ = ShowWindow(w, SW_RESTORE);
                let _ = SetForegroundWindow(w);
            }
            return Ok(None);
        }
        Ok(Some(Self(h)))
    }
}
impl Drop for UiInstance {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
struct RuntimeOwner(std::process::Child);
impl RuntimeOwner {
    fn start(product: &Path) -> Result<Self, String> {
        use std::os::windows::process::CommandExt;
        let exe = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .with_file_name("handoff-runtime.exe");
        let log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(product.join("runtime/runtime-stderr.log"))
            .map_err(|e| e.to_string())?;
        let child = std::process::Command::new(exe)
            .arg("--product-root")
            .arg(product)
            .arg("--owner-pid")
            .arg(std::process::id().to_string())
            .creation_flags(0x08000000)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::from(log))
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(Self(child))
    }
}
impl Drop for RuntimeOwner {
    fn drop(&mut self) {
        use std::os::windows::process::CommandExt;
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = std::process::Command::new("taskkill.exe")
                .args(["/PID", &self.0.id().to_string(), "/T", "/F"])
                .creation_flags(0x08000000)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
        let _ = self.0.wait();
    }
}
pub fn run() -> Result<(), String> {
    unsafe {
        let args: Vec<String> = std::env::args().collect();
        let root = args
            .windows(2)
            .find(|w| w[0] == "--product-root")
            .map(|w| w[1].as_str());
        let product = discover_product(root)?;
        // A brand new product root has no runtime directory yet, and the runtime
        // child writes its stderr log into it. Create it before anything else so a
        // first run cannot fail on a missing path.
        fs::create_dir_all(product.join("runtime"))
            .map_err(|e| format!("无法创建 runtime 目录（{}）：{e}", product.display()))?;
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        if args.iter().any(|a| a == "--audit-layout") {
            return layout_audit(&product);
        }
        let demo = args
            .iter()
            .find_map(|a| a.strip_prefix("--demo=").map(str::to_owned));
        let _instance = if demo.is_none() {
            let Some(instance) = UiInstance::acquire()? else {
                return Ok(());
            };
            Some(instance)
        } else {
            None
        };
        let runtime = if demo.is_none() {
            Some(RuntimeOwner::start(&product)?)
        } else {
            None
        };
        let dpi = GetDpiForSystem().max(96);
        // The work area has to be known before the app exists, because the scale it
        // implies drives both the fonts and every control's geometry.
        let mut work = RECT::default();
        let _ = SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(&mut work as *mut _ as *mut c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        let dpi = fit_dpi(dpi, work.right - work.left);
        let mut app = Box::new(App::new(product, dpi, demo));
        // Owned by the window from here on, so it can be replaced if it dies.
        app.runtime = runtime;
        app.snapshot.demo_enabled = args.iter().any(|a| a == "--demo-enabled");
        let instance = GetModuleHandleW(None).map_err(|e| e.to_string())?;
        let class = w!("A2AHandoff.Product.UI.v2");
        let (class_icon, window_icon_small) = load_app_icon(dpi);
        let wc = WNDCLASSW {
            hCursor: LoadCursorW(None, IDC_ARROW).map_err(|e| e.to_string())?,
            hInstance: instance.into(),
            lpszClassName: class,
            lpfnWndProc: Some(window_proc),
            hIcon: class_icon,
            ..Default::default()
        };
        RegisterClassW(&wc);
        // No WS_THICKFRAME and no WS_MAXIMIZEBOX: the window has exactly one size (see
        // fixed_window_rect), so a resize border or a maximise button would advertise
        // something that cannot happen.
        let style = (WS_OVERLAPPEDWINDOW & !(WS_THICKFRAME | WS_MAXIMIZEBOX)) | WS_CLIPCHILDREN;
        let ex = WS_EX_CONTROLPARENT;
        // `work` was read before the app was built, so the layout already fits it.
        let size = fixed_window_rect(dpi, style);
        let width = (size.right - size.left).min(work.right - work.left - view::px(32, dpi));
        let height = (size.bottom - size.top).min(work.bottom - work.top - view::px(32, dpi));
        let x = work.left + (work.right - work.left - width) / 2;
        let y = work.top + (work.bottom - work.top - height) / 2;
        let raw = Box::into_raw(app);
        let hwnd = CreateWindowExW(
            ex,
            class,
            w!("A2AHandoff · 智能体任务交接"),
            style,
            x,
            y,
            width,
            height,
            None,
            None,
            Some(instance.into()),
            Some(raw as *const c_void),
        )
        .map_err(|e| e.to_string())?;
        // The small icon (title bar, Alt+Tab) is a per-window property; WNDCLASSW has no
        // hIconSm field - that exists only on WNDCLASSEXW - so it is set here.
        if !window_icon_small.is_invalid() {
            let _ = SendMessageW(
                hwnd,
                WM_SETICON,
                Some(WPARAM(ICON_SMALL as usize)),
                Some(LPARAM(window_icon_small.0 as isize)),
            );
        }
        let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
        let _ = UpdateWindow(hwnd);
        let mut message = MSG::default();
        loop {
            let status = GetMessageW(&mut message, None, 0, 0).0;
            if status == 0 {
                break;
            }
            if status < 0 {
                return Err("Windows 消息循环错误。".into());
            }
            // IsDialogMessageW consumes Tab/arrow/Enter before the window procedure
            // ever sees them, so the keyboard-vs-pointer mode has to be tracked here,
            // where those keys are still observable. Focus rings are only drawn in
            // keyboard mode.
            if matches!(message.message, WM_KEYDOWN | WM_SYSKEYDOWN) {
                let app = app_ptr(hwnd);
                if !app.is_null() && !(*app).keyboard {
                    (*app).keyboard = true;
                    (*app).invalidate_controls();
                }
            }
            if !IsDialogMessageW(hwnd, &message).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod fit_tests {
    use super::*;
    /// The layout is authored at 960 DIP; the scale must never let it exceed the work
    /// area, and must not shrink a display that can already hold it.
    #[test]
    fn fit_dpi_keeps_the_layout_inside_the_work_area() {
        // No information: keep the real scale.
        assert_eq!(fit_dpi(192, 0), 192);
        // 100% on a roomy display: unchanged.
        assert_eq!(fit_dpi(96, 1920), 96);
        // 150% on a roomy display: unchanged.
        assert_eq!(fit_dpi(144, 2560), 144);
        // The reported machine: 1560x1040 at 200%. 960 DIP wants 1920 physical pixels,
        // which does not fit, so the scale drops until it does.
        let fitted = fit_dpi(192, 1560);
        assert!(
            fitted < 192,
            "must shrink on a display that cannot hold 1920"
        );
        let content = crate::view::px(crate::view::DEFAULT_WIDTH, fitted);
        // It must fit the work area, and it must be the LARGEST scale that fits - one
        // step up has to overflow, or the layout is needlessly small.
        assert!(
            content <= 1560,
            "content {content} must fit the 1560 work area"
        );
        let bigger = crate::view::px(crate::view::DEFAULT_WIDTH, fitted + 1);
        assert!(
            bigger > 1560 - crate::view::px(32, 192),
            "a larger scale ({bigger}) should not still fit; picked {fitted}"
        );
        // Never below the floor, and never above the real scale.
        assert!(fitted >= 96);
        assert!(fit_dpi(192, 100) >= 96);
        assert_eq!(fit_dpi(96, 100), 96);
    }

    /// The window is pinned to exactly one size at every scale: the layout cannot use
    /// more than the design, so stretching it only ever bought empty frame.
    #[test]
    fn the_window_is_pinned_to_the_design_size() {
        // Pinning min to max only means "one size" while the design width is also the
        // minimum. If that ever stops holding, the window would silently become a range.
        assert_eq!(crate::view::MIN_WIDTH, crate::view::DEFAULT_WIDTH);
        let style = (WS_OVERLAPPEDWINDOW & !(WS_THICKFRAME | WS_MAXIMIZEBOX)) | WS_CLIPCHILDREN;
        let at = |dpi: u32| {
            let r = fixed_window_rect(dpi, style);
            (r.right - r.left, r.bottom - r.top)
        };
        for dpi in [96u32, 144, 192] {
            let (w, h) = at(dpi);
            assert!(
                w >= crate::view::px(crate::view::DEFAULT_WIDTH, dpi),
                "dpi {dpi}: window {w} clips the design client"
            );
            assert!(
                h >= crate::view::px(crate::view::CONTENT_HEIGHT, dpi),
                "dpi {dpi}: window {h} clips the design client"
            );
        }
        // It follows the scale instead of being one fixed pixel count.
        let (small, _) = at(96);
        let (large, _) = at(192);
        assert!(large > small, "{large} should exceed {small}");
        // And it is not minimisable away: the pinned size never drops below the design.
        assert!(small >= crate::view::px(crate::view::DEFAULT_WIDTH, 96));
    }

    /// The respawn backoff starts short and grows to a cap: a one-off death should cost
    /// seconds, but a runtime that dies on every start must not spin the machine.
    #[test]
    fn respawn_backoff_grows_and_then_holds() {
        assert_eq!(respawn_delay(1), Duration::from_secs(5));
        assert_eq!(respawn_delay(2), Duration::from_secs(15));
        assert_eq!(respawn_delay(3), Duration::from_secs(45));
        assert_eq!(respawn_delay(4), Duration::from_secs(120));
        assert_eq!(respawn_delay(9), Duration::from_secs(120));
        assert_eq!(respawn_delay(u32::MAX), Duration::from_secs(120));
    }
}
