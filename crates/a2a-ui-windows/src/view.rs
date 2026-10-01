//! Platform-independent layout in DIP (100% scale). Produces a display list
//! (`Prim`) and a control list (`Control`); `win.rs` only paints and hosts it.
//! Product/UI behavior is documented under `docs/`; this module contains the native view model.
use crate::model::Snapshot;

pub const DEFAULT_WIDTH: i32 = 960;
/// Minimum client height (spec §3). Content is 452 tall; the rest is intentional slack.
pub const CONTENT_HEIGHT: i32 = 480;
pub const MIN_WIDTH: i32 = 960;
pub const NOTICE_MS: u64 = 4000;

pub const ID_SEND_DSH: usize = 101;
pub const ID_SEND_CLAUDE: usize = 102;
pub const ID_CANCEL: usize = 103;
pub const ID_MINUS: usize = 201;
pub const ID_VALUE: usize = 202;
pub const ID_PLUS: usize = 203;
pub const ID_SAVE: usize = 204;
pub const ID_BINDINGS: usize = 301;
pub const ID_LOGS: usize = 302;
pub const ID_TOGGLE: usize = 303;
pub const ID_TEMPLATES: usize = 304;
pub const ID_RESTORE: usize = 305;
pub const ID_BANNER_LOGS: usize = 306;
pub const ID_BANNER_POLL: usize = 308;
pub const ID_BANNER_RETRY: usize = 310;
pub const ID_SETTINGS: usize = 309;

pub const MIN_POLL: u64 = 1;
pub const MAX_POLL: u64 = 120;

/// Design tokens (spec §2.1), 0xAARRGGBB.
pub mod tok {
    pub const GROUND: u32 = 0xFF121820;
    pub const PANEL: u32 = 0xFF18202A;
    pub const RAISED: u32 = 0xFF202A36;
    pub const RAISED_HOVER: u32 = 0xFF263241;
    pub const RAISED_PRESSED: u32 = 0xFF1B2430;
    pub const LINE: u32 = 0xFF2B3643;
    pub const LINE_STRONG: u32 = 0xFF3A4757;
    pub const TEXT: u32 = 0xFFE7ECF2;
    pub const TEXT2: u32 = 0xFFA8B3C1;
    pub const TEXT3: u32 = 0xFF7F8B9A;
    pub const TEXT_DISABLED: u32 = 0xFF5C6776;
    pub const CLAUDE: u32 = 0xFFD98A4E;
    pub const CLAUDE_HOVER: u32 = 0xFFE39A62;
    pub const CLAUDE_PRESSED: u32 = 0xFFC57A40;
    pub const CLAUDE_ON: u32 = 0xFF1F1206;
    pub const CLAUDE_SOFT: u32 = 0xFFEBA673;
    pub const CLAUDE_TINT: u32 = 0x24D98A4E;
    pub const DSH: u32 = 0xFF3563E9;
    pub const DSH_HOVER: u32 = 0xFF4572F0;
    pub const DSH_PRESSED: u32 = 0xFF2A55D0;
    pub const DSH_ACCENT: u32 = 0xFF4D7CFE;
    pub const DSH_SOFT: u32 = 0xFF86A8FF;
    pub const DSH_TINT: u32 = 0x244D7CFE;
    pub const OK: u32 = 0xFF3DB88A;
    pub const OK_TEXT: u32 = 0xFF5FD0A4;
    pub const OK_TINT: u32 = 0x1F3DB88A;
    pub const WARN: u32 = 0xFFE3A83E;
    pub const WARN_TEXT: u32 = 0xFFF0C067;
    pub const ERROR: u32 = 0xFFEF6B6B;
    pub const WHITE: u32 = 0xFFFFFFFF;
}
use tok::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Box2 {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}
impl Box2 {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }
}

/// Type scale (spec §2.2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Font {
    Title,
    Round,
    Name,
    Avatar,
    Task,
    BannerTitle,
    Body,
    BodyStrong,
    Label,
    Small,
    Code,
    Glyph,
}
impl Font {
    pub const ALL: [Font; 12] = [
        Font::Title,
        Font::Round,
        Font::Name,
        Font::Avatar,
        Font::Task,
        Font::BannerTitle,
        Font::Body,
        Font::BodyStrong,
        Font::Label,
        Font::Small,
        Font::Code,
        Font::Glyph,
    ];
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|f| *f == self).unwrap_or(0)
    }
    pub fn size(self) -> i32 {
        match self {
            Self::Title => 18,
            Self::Round => 28,
            Self::Name => 15,
            Self::Avatar | Self::Glyph => 16,
            Self::Task | Self::BannerTitle => 14,
            Self::Body | Self::BodyStrong | Self::Label => 13,
            Self::Small => 12,
            Self::Code => 11,
        }
    }
    pub fn weight(self) -> i32 {
        match self {
            Self::Title | Self::Round | Self::Name | Self::BannerTitle => 600,
            Self::BodyStrong => 600,
            Self::Task | Self::Label => 500,
            // T5 polish: the single avatar letter looked too light at 600 because the
            // Semibold face is selected at weight 400 (see `Fonts::pick`); ask for Bold.
            Self::Avatar => 700,
            _ => 400,
        }
    }
    #[allow(dead_code)]
    /// Line box height (line-height 1.4, except title/round = 1.0).
    pub fn line(self) -> i32 {
        match self {
            Self::Title => 18,
            Self::Round => 28,
            f => (f.size() * 14 + 5) / 10,
        }
    }
}

/// Text measurement supplied by the host (GDI on Windows, an estimate in tests).
pub trait Measure {
    fn text_w(&self, s: &str, f: Font) -> i32;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Align {
    Left,
    Center,
    Right,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Icon {
    Swap,
    Link,
    Terminal,
    Send,
    Check,
    Warn,
    Clock,
    Refresh,
    Doc,
    CircleX,
    CircleCheck,
    Pause,
}
impl Icon {
    /// Stroke width in 24-unit viewBox space (spec §2.3).
    pub fn stroke(self) -> f32 {
        match self {
            Self::Send | Self::Check => 2.0,
            _ => 1.75,
        }
    }
    fn paths(self) -> &'static [&'static str] {
        match self {
            Self::Swap => &["M7 4 3 8l4 4", "M3 8h14", "m17 20 4-4-4-4", "M21 16H7"],
            Self::Link => &[
                "M10 13a5 5 0 0 0 7.5.5l3-3a5 5 0 0 0-7-7l-1.7 1.7",
                "M14 11a5 5 0 0 0-7.5-.5l-3 3a5 5 0 0 0 7 7l1.7-1.7",
            ],
            Self::Terminal => &["m4 17 6-6-6-6", "M12 19h8"],
            Self::Send => &["m22 2-7 20-4-9-9-4Z", "M22 2 11 13"],
            Self::Check => &["M20 6 9 17l-5-5"],
            Self::Warn => &[
                "M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0Z",
                "M12 9v4",
                "M12 17h.01",
            ],
            Self::Clock => &["M3 12a9 9 0 1 0 18 0a9 9 0 1 0-18 0", "M12 7v5l3 2"],
            Self::Refresh => &["M21 12a9 9 0 1 1-2.6-6.4L21 8", "M21 3v5h-5"],
            Self::Doc => &[
                "M14 3H6a1 1 0 0 0-1 1v16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1V8Z",
                "M14 3v5h5",
                "M9 13h6",
                "M9 17h4",
            ],
            Self::CircleX => &[
                "M3 12a9 9 0 1 0 18 0a9 9 0 1 0-18 0",
                "m15 9-6 6",
                "m9 9 6 6",
            ],
            Self::CircleCheck => &["M3 12a9 9 0 1 0 18 0a9 9 0 1 0-18 0", "m9 12 2 2 4-4"],
            Self::Pause => &["M9 5v14", "M15 5v14"],
        }
    }
    /// Flattened polylines in viewBox coordinates.
    pub fn strokes(self) -> Vec<Vec<(f32, f32)>> {
        self.paths().iter().flat_map(|d| flatten_path(d)).collect()
    }
}

// ---- minimal SVG path flattening (M L H V A Z, absolute + relative) ----
struct Scan<'a> {
    b: &'a [u8],
    i: usize,
}
impl Scan<'_> {
    fn skip(&mut self) {
        while self.i < self.b.len() && (self.b[self.i] == b' ' || self.b[self.i] == b',') {
            self.i += 1;
        }
    }
    fn more(&mut self) -> bool {
        self.skip();
        self.i < self.b.len() && !self.b[self.i].is_ascii_alphabetic()
    }
    fn num(&mut self) -> f32 {
        self.skip();
        let s = self.i;
        if self.i < self.b.len() && (self.b[self.i] == b'-' || self.b[self.i] == b'+') {
            self.i += 1;
        }
        let mut dot = false;
        while self.i < self.b.len() {
            let c = self.b[self.i];
            if c.is_ascii_digit() {
                self.i += 1;
            } else if c == b'.' && !dot {
                dot = true;
                self.i += 1;
            } else {
                break;
            }
        }
        std::str::from_utf8(&self.b[s..self.i])
            .ok()
            .and_then(|t| t.parse().ok())
            .unwrap_or(0.0)
    }
    fn flag(&mut self) -> bool {
        self.skip();
        let v = self.i < self.b.len() && self.b[self.i] == b'1';
        self.i += 1;
        v
    }
}
fn arc_points(
    from: (f32, f32),
    mut rx: f32,
    mut ry: f32,
    rot: f32,
    large: bool,
    sweep: bool,
    to: (f32, f32),
) -> Vec<(f32, f32)> {
    if rx == 0.0 || ry == 0.0 || from == to {
        return vec![to];
    }
    rx = rx.abs();
    ry = ry.abs();
    let phi = rot.to_radians();
    let (cp, sp) = (phi.cos(), phi.sin());
    let dx = (from.0 - to.0) / 2.0;
    let dy = (from.1 - to.1) / 2.0;
    let x1 = cp * dx + sp * dy;
    let y1 = -sp * dx + cp * dy;
    let lam = x1 * x1 / (rx * rx) + y1 * y1 / (ry * ry);
    if lam > 1.0 {
        let s = lam.sqrt();
        rx *= s;
        ry *= s;
    }
    let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut co = if den == 0.0 {
        0.0
    } else {
        (num / den).max(0.0).sqrt()
    };
    if large == sweep {
        co = -co;
    }
    let cxp = co * rx * y1 / ry;
    let cyp = -co * ry * x1 / rx;
    let cx = cp * cxp - sp * cyp + (from.0 + to.0) / 2.0;
    let cy = sp * cxp + cp * cyp + (from.1 + to.1) / 2.0;
    let ang = |ux: f32, uy: f32, vx: f32, vy: f32| {
        let d = (ux * vx + uy * vy) / ((ux * ux + uy * uy).sqrt() * (vx * vx + vy * vy).sqrt());
        let mut a = d.clamp(-1.0, 1.0).acos();
        if ux * vy - uy * vx < 0.0 {
            a = -a;
        }
        a
    };
    let t1 = ang(1.0, 0.0, (x1 - cxp) / rx, (y1 - cyp) / ry);
    let mut dt = ang(
        (x1 - cxp) / rx,
        (y1 - cyp) / ry,
        (-x1 - cxp) / rx,
        (-y1 - cyp) / ry,
    );
    if !sweep && dt > 0.0 {
        dt -= std::f32::consts::TAU;
    } else if sweep && dt < 0.0 {
        dt += std::f32::consts::TAU;
    }
    let n = ((dt.abs() / (std::f32::consts::PI / 24.0)).ceil() as usize).max(4);
    (1..=n)
        .map(|k| {
            let t = t1 + dt * k as f32 / n as f32;
            let (x, y) = (rx * t.cos(), ry * t.sin());
            (cp * x - sp * y + cx, sp * x + cp * y + cy)
        })
        .collect()
}
pub fn flatten_path(d: &str) -> Vec<Vec<(f32, f32)>> {
    let mut sc = Scan {
        b: d.as_bytes(),
        i: 0,
    };
    let mut out: Vec<Vec<(f32, f32)>> = Vec::new();
    let (mut cur, mut start) = ((0.0f32, 0.0f32), (0.0f32, 0.0f32));
    let mut cmd = b'M';
    while {
        sc.skip();
        sc.i < sc.b.len()
    } {
        if sc.b[sc.i].is_ascii_alphabetic() {
            cmd = sc.b[sc.i];
            sc.i += 1;
            if cmd == b'Z' || cmd == b'z' {
                if let Some(last) = out.last_mut() {
                    last.push(start);
                }
                cur = start;
                continue;
            }
        } else if cmd == b'M' {
            cmd = b'L';
        } else if cmd == b'm' {
            cmd = b'l';
        }
        let rel = cmd.is_ascii_lowercase();
        let (ox, oy) = if rel { cur } else { (0.0, 0.0) };
        match cmd.to_ascii_uppercase() {
            b'M' => {
                let p = (sc.num() + ox, sc.num() + oy);
                out.push(vec![p]);
                cur = p;
                start = p;
            }
            b'L' => {
                let p = (sc.num() + ox, sc.num() + oy);
                if let Some(last) = out.last_mut() {
                    last.push(p);
                }
                cur = p;
            }
            b'H' => {
                let p = (sc.num() + ox, cur.1);
                if let Some(last) = out.last_mut() {
                    last.push(p);
                }
                cur = p;
            }
            b'V' => {
                let p = (cur.0, sc.num() + oy);
                if let Some(last) = out.last_mut() {
                    last.push(p);
                }
                cur = p;
            }
            b'A' => {
                let (rx, ry, rot) = (sc.num(), sc.num(), sc.num());
                let (large, sweep) = (sc.flag(), sc.flag());
                let to = (sc.num() + ox, sc.num() + oy);
                let pts = arc_points(cur, rx, ry, rot, large, sweep, to);
                if let Some(last) = out.last_mut() {
                    last.extend(pts);
                }
                cur = to;
            }
            _ => break,
        }
        let _ = sc.more();
    }
    out
}

// ---- display list ----
#[derive(Clone, Debug, PartialEq)]
pub enum Prim {
    Rect {
        a: Box2,
        r: i32,
        fill: u32,
        stroke: Option<u32>,
    },
    Text {
        a: Box2,
        s: String,
        f: Font,
        color: u32,
        align: Align,
    },
    Icon {
        a: Box2,
        k: Icon,
        color: u32,
    },
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    PrimaryClaude,
    PrimaryDsh,
    Secondary,
    Ghost,
    /// Secondary button inside a banner (stronger border).
    BannerSecondary,
    Switch(bool),
    Step,
    Edit,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Control {
    pub id: usize,
    pub area: Box2,
    pub label: String,
    pub kind: Kind,
    pub enabled: bool,
    pub icon: Option<Icon>,
    pub loading: bool,
}
#[derive(Default, Clone, Debug, PartialEq)]
pub struct View {
    pub prims: Vec<Prim>,
    pub controls: Vec<Control>,
    pub tips: Vec<(Box2, String)>,
}
impl View {
    fn rect(&mut self, a: Box2, r: i32, fill: u32, stroke: Option<u32>) {
        self.prims.push(Prim::Rect { a, r, fill, stroke });
    }
    fn text(&mut self, a: Box2, s: impl Into<String>, f: Font, color: u32, align: Align) {
        self.prims.push(Prim::Text {
            a,
            s: s.into(),
            f,
            color,
            align,
        });
    }
    fn icon(&mut self, a: Box2, k: Icon, color: u32) {
        self.prims.push(Prim::Icon { a, k, color });
    }
    #[allow(clippy::too_many_arguments)]
    fn ctl(
        &mut self,
        id: usize,
        area: Box2,
        label: &str,
        kind: Kind,
        enabled: bool,
        icon: Option<Icon>,
        loading: bool,
    ) {
        self.controls.push(Control {
            id,
            area,
            label: label.into(),
            kind,
            enabled,
            icon,
            loading,
        });
    }
}

// ---- state → UI mapping (spec §6) ----
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Variant {
    Warn,
    Error,
    Success,
    Info,
    Neutral,
}
impl Variant {
    /// (background, border, icon colour, icon)
    fn look(self) -> (u32, u32, u32, Icon) {
        match self {
            Self::Warn => (0x14E3A83E, 0x52E3A83E, WARN, Icon::Warn),
            Self::Error => (0x14EF6B6B, 0x52EF6B6B, ERROR, Icon::CircleX),
            Self::Success => (0x143DB88A, 0x523DB88A, OK, Icon::CircleCheck),
            Self::Info => (0x144D7CFE, 0x524D7CFE, DSH_ACCENT, Icon::Clock),
            Self::Neutral => (PANEL, LINE, TEXT3, Icon::Pause),
        }
    }
}
/// Transient message (4 s) shown in the banner slot.
#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    pub variant: Variant,
    pub title: String,
    pub detail: String,
}
#[derive(Clone, Debug, PartialEq)]
pub struct BannerButton {
    pub id: usize,
    pub label: &'static str,
    pub icon: Option<Icon>,
    pub ghost: bool,
    pub enabled: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Banner {
    pub variant: Variant,
    pub title: String,
    pub desc: String,
    pub code: Option<String>,
    pub buttons: Vec<BannerButton>,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tone {
    Ok,
    Warn,
    Muted,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Ui {
    pub available: bool,
    pub listening: bool,
    /// Which listener state the window should report, in the product's own names.
    pub listener: ListenerState,
    pub auto_on: bool,
    pub round: Option<u64>,
    pub claude_bound: bool,
    pub claude_task: String,
    pub claude_status: (Tone, String),
    pub dsh_task: String,
    pub dsh_sub: String,
    pub dsh_status: (Tone, String),
    pub send_dsh: bool,
    pub send_claude: bool,
    pub loading_dsh: bool,
    pub loading_claude: bool,
    pub toggle_listen_enabled: bool,
    pub toggle_auto_enabled: bool,
    pub banner: Banner,
    /// None = paused / unknown, Some((seconds left, elapsed percent 0..=100))
    pub poll: Option<(u64, u32)>,
}

fn live_str<'a>(s: &'a Snapshot, k: &str) -> &'a str {
    s.live[k].as_str().unwrap_or("")
}
fn view_log_button() -> BannerButton {
    BannerButton {
        id: ID_BANNER_LOGS,
        label: "查看日志",
        icon: None,
        ghost: true,
        enabled: true,
    }
}
/// Enabled: pressing it sends `poll_now`, which re-reads both sessions without
/// touching the workflow baseline (`derive` still gates it on the idle state).
fn poll_button() -> BannerButton {
    BannerButton {
        id: ID_BANNER_POLL,
        label: "立即轮询",
        icon: Some(Icon::Refresh),
        ghost: false,
        enabled: true,
    }
}
/// Offered only while a delivery is held AND the peer cannot have received anything, so a
/// retry cannot duplicate a message. The runtime enforces the same rule (`retry_direction`);
/// this just keeps the button off the screen when it would be refused.
fn retry_button() -> BannerButton {
    BannerButton {
        id: ID_BANNER_RETRY,
        label: "重试本次投递",
        icon: None,
        ghost: false,
        enabled: true,
    }
}
/// The one hold the user can clear themselves.
///
/// The adapter refuses to overwrite a target input that already has content, and the bare
/// code (`DRAFT_OCCUPIED_PRESERVED`) says nothing about what to do about it - so the banner
/// says it.
fn occupied_hint(code: &str) -> String {
    if code.starts_with("DRAFT_OCCUPIED") || code == "EXISTING_ATTACHMENT_PRESERVED" {
        "目标输入框已有草稿，已保留；清空后再点重试。".to_owned()
    } else {
        String::new()
    }
}
/// Text after the first full-width or ASCII colon, i.e. the raw error string.
fn error_code(detail: &str) -> String {
    let cut = detail
        .find('：')
        .map(|i| i + '：'.len_utf8())
        .or_else(|| detail.find(": ").map(|i| i + 2));
    let t = match cut {
        Some(i) if detail.len() > i => &detail[i..],
        _ => detail,
    };
    t.trim().to_owned()
}

/// What the listener is doing, in the state names the product specifies.
///
/// A single boolean could not express this: "心跳过期" and "会话不匹配" are different
/// problems from "尚未绑定", and the window previously collapsed all of them into
/// one hardcoded 「监听中 / 未监听」 (or, before that, 「已停止监听」 while the banner
/// said the opposite). The snapshot already carries `watch_age`,
/// `watch_session_matches`, `dsh_turn` and `reply_turn`, so every state below is
/// derived from data the runtime actually publishes rather than guessed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListenerState {
    /// No conversation has been bound yet.
    SessionUnavailable,
    /// The bound session is not the one the runtime is reading.
    SessionMismatch,
    /// The runtime heartbeat has gone stale.
    HeartbeatExpired,
    /// Attached to the round that is currently in flight.
    TurnConnected,
    /// Baselines taken; waiting for the next round to begin.
    WaitingNextTurn,
    /// Watching and idle - nothing is pending on either side.
    Normal,
    /// Automatic handoff is paused; observation carries on.
    Paused,
}

impl ListenerState {
    /// Full description, for the status area.
    pub fn label(self) -> &'static str {
        match self {
            ListenerState::SessionUnavailable => "会话不可读取",
            ListenerState::SessionMismatch => "会话不匹配",
            ListenerState::HeartbeatExpired => "心跳过期",
            ListenerState::TurnConnected => "当前轮已连接",
            ListenerState::WaitingNextTurn => "等待下一轮",
            ListenerState::Normal => "监听正常",
            ListenerState::Paused => "自动交接已暂停",
        }
    }
    /// Compact description, for the small card badge.
    pub fn short(self) -> &'static str {
        match self {
            ListenerState::SessionUnavailable => "不可读",
            ListenerState::SessionMismatch => "不匹配",
            ListenerState::HeartbeatExpired => "心跳过期",
            ListenerState::TurnConnected => "已连接",
            ListenerState::WaitingNextTurn => "待下一轮",
            ListenerState::Normal => "监听中",
            ListenerState::Paused => "已暂停",
        }
    }
}

pub fn derive(s: &Snapshot, watch_alive: bool, notice: Option<&Notice>, now: u64) -> Ui {
    let demo = s.demo.as_deref();
    let live_ok = s.live["mode"] == "live";
    let available =
        demo.is_none() && watch_alive && s.runtime_age.is_some_and(|a| a < 45) && live_ok;
    let phase = live_str(s, "phase").to_owned();
    let sending = s.live["sending"].as_bool().unwrap_or(false);
    let pending = !s.live["pending"].is_null();
    let stage = s.live["pending"]["stage"].as_str().unwrap_or("").to_owned();
    let holding = phase == "hold_send_uncertain";
    let cfg_enabled = s.live["enabled"].as_bool().unwrap_or(false);
    // `listening` reports whether the runtime is still WATCHING both sessions, which
    // it does in every phase once it is bound: observation keeps running, the
    // snapshot keeps refreshing, and manual send keeps working. Pausing only stops
    // AUTOMATIC sends, which `auto_on` already expresses.
    //
    // Only `unclaimed` really is not listening - nothing is bound yet, so the runtime
    // is not following either session. Previously `paused_by_user` and the two hold
    // phases were lumped in here too, which made the header pill read
    // 「V1 已停止监听」 in the very phase whose banner said 「监听仍在」.
    let listening = match demo {
        Some(d) => !matches!(d, "stopped" | "manual"),
        None => available && !matches!(phase.as_str(), "" | "unclaimed"),
    };
    // The listener's own state, in the names the product specifies. Ordered by which
    // fact dominates: an unreadable or mismatched session makes every other claim
    // meaningless, and a stale heartbeat means we cannot claim to be watching at all.
    let listener = if demo.is_some() {
        if listening {
            ListenerState::Normal
        } else {
            ListenerState::Paused
        }
    } else if !watch_alive {
        ListenerState::HeartbeatExpired
    } else if !s.watch_session_matches {
        ListenerState::SessionMismatch
    } else if phase.is_empty() || phase == "unclaimed" {
        ListenerState::SessionUnavailable
    } else if !available {
        ListenerState::HeartbeatExpired
    } else if matches!(phase.as_str(), "paused_by_user" | "hold_preparation") {
        ListenerState::Paused
    } else if phase == "hold_send_uncertain" {
        ListenerState::WaitingNextTurn
    } else if phase == "waiting_dsh" {
        // `waiting_dsh` covers both "DSH owes us a round" and "we handed the result to
        // Claude and Claude owes the answer". Reporting 等待下一轮 for the second case
        // would name the wrong side, so the direction of the last handover decides.
        let handed_to_claude =
            s.live["last_delivery"]["direction"].as_str() == Some("DSH_TO_CLAUDE");
        if handed_to_claude {
            ListenerState::Normal
        } else {
            // Attached to the round in flight, or simply waiting for the next one.
            let caught_up = s
                .reply_turn
                .zip(s.dsh_turn)
                .is_none_or(|(reply, turn)| reply >= turn);
            if caught_up {
                ListenerState::WaitingNextTurn
            } else {
                ListenerState::TurnConnected
            }
        }
    } else {
        ListenerState::Normal
    };
    let auto_on = match demo {
        Some(d) => !matches!(d, "stopped" | "manual" | "paused" | "cancelled"),
        None => listening && cfg_enabled,
    };
    // The 监听 switch is the old 「恢复监听」 action, which is one-way: the runner has
    // no stop-listening command. While the listener is already on it must therefore
    // be a read-only state display, or it looks clickable but does nothing. It is
    // only offered where there is actually something to restore.
    let needs_restore = matches!(
        phase.as_str(),
        "unclaimed" | "paused_by_user" | "hold_send_uncertain" | "hold_preparation"
    );
    let claude_bound = !s.claude_session.is_empty() && !s.claude_title.is_empty();
    let claude_ok = s.live["claude_ok"].as_bool();
    let dsh_ok = s.live["dsh_ok"].as_bool();
    let demo_paused = demo == Some("paused");
    let claude_read_failed = demo_paused || (available && claude_ok == Some(false));
    let dsh_read_failed = available && dsh_ok == Some(false);
    let claude_status = if !claude_bound {
        (Tone::Muted, "尚未绑定窗口".to_owned())
    } else if claude_read_failed {
        if demo_paused {
            (Tone::Warn, "绑定已保存，会话待核验".into())
        } else {
            (Tone::Warn, "读取失败，详见下方提示".into())
        }
    } else if claude_ok == Some(true) || matches!(demo, Some(d) if d != "paused") {
        (Tone::Ok, "会话已核验，正在写入交接草稿".into())
    } else {
        (Tone::Warn, "绑定已保存，会话待核验".into())
    };
    let dsh_sub = match (s.dsh_turn, s.dsh_running) {
        (Some(n), Some(true)) => format!("第 {n} 轮进行中"),
        (Some(n), Some(false)) => format!("第 {n} 轮已结束"),
        _ => "原生轮次暂不可读".into(),
    };
    let dsh_status = if auto_on {
        (Tone::Ok, "自动交接已开启，收到新回复即开始".to_owned())
    } else {
        (Tone::Muted, "自动交接已暂停，需手动发送".into())
    };
    let idle_gate = available && !sending && !pending && !holding;
    let send_dsh = idle_gate && s.dsh_running == Some(false);
    let send_claude = idle_gate && s.reply_turn.is_some();
    let to_claude = s.live["pending"]["direction"] == "DSH_TO_CLAUDE";

    // ---- banner, priority error > warn > success > info > neutral ----
    // A transient notice REPLACES THE TEXT ONLY. The banner is a single slot, so
    // swapping the whole banner used to hide the state's action buttons (「立即轮询」
    // etc.) for the notice's 4 s lifetime, which read as the button disappearing.
    let mut banner = if demo == Some("countdown")
        || (pending && stage == "draft_ready")
        || (pending && !sending)
    {
        let (title, desc) = if demo.is_some() {
            let left = s
                .demo_deadline_ms
                .unwrap_or(0)
                .saturating_sub(now)
                .div_ceil(1000);
            (
                format!("DSH → Claude · {left} 秒"),
                "界面演示，可取消；倒计时结束也不会真实发送。".to_owned(),
            )
        } else {
            (
                live_str(s, "status_text").to_owned(),
                live_str(s, "detail").to_owned(),
            )
        };
        Banner {
            variant: Variant::Info,
            title,
            desc,
            code: None,
            buttons: {
                let mut b = vec![BannerButton {
                    id: ID_CANCEL,
                    label: "取消本次",
                    icon: None,
                    ghost: false,
                    enabled: true,
                }];
                // A delivery held because the target input was occupied is still queued, so it
                // can go as soon as the user clears the box. The runtime keeps the pending for
                // exactly this (`preparation_failed`) and a click clears the backoff it was
                // waiting out - without the button the only way on was 取消本次 or 监听.
                if phase == "hold_preparation" {
                    b.insert(0, retry_button());
                }
                b
            },
        }
    } else if sending {
        Banner {
            variant: Variant::Info,
            title: live_str(s, "status_text").to_owned(),
            desc: live_str(s, "detail").to_owned(),
            code: None,
            buttons: vec![BannerButton {
                id: ID_CANCEL,
                label: "取消本次",
                icon: None,
                ghost: false,
                enabled: pending,
            }],
        }
    } else if holding || demo == Some("error") {
        let target = if s.live["last_delivery"]["direction"] == "DSH_TO_CLAUDE" {
            "Claude"
        } else {
            "DSH"
        };
        let code = if demo == Some("error") {
            "SEND_UNCERTAIN: composer not confirmed".to_owned()
        } else {
            error_code(live_str(s, "detail"))
        };
        Banner {
            variant: Variant::Error,
            title: if demo == Some("error") {
                format!("发送给 {target} 失败")
            } else {
                live_str(s, "status_text").to_owned()
            },
            desc: occupied_hint(&code),
            code: Some(code),
            buttons: {
                let mut b = vec![view_log_button()];
                // The hold is deliberate: the runtime stopped rather than resend blind, and
                // with `hold_send_uncertain` the send buttons are disabled, which left no way
                // back to this particular handoff except re-baselining (which skips it).
                // A draft the adapter never verified cannot have reached the peer, so it is
                // the one case that may safely be re-run.
                if s.live["last_delivery"]["state"] == "draft_unverified"
                    && matches!(
                        s.live["last_delivery"]["direction"].as_str(),
                        Some("DSH_TO_CLAUDE" | "CLAUDE_TO_DSH")
                    )
                {
                    b.insert(0, retry_button());
                }
                b
            },
        }
    } else if demo_paused
        || claude_read_failed
        || dsh_read_failed
        || (available && live_str(s, "status_text") == "交接暂缓，等待核验")
    {
        let detail = live_str(s, "detail");
        let (title, hint) = if claude_read_failed {
            (
                "交接已暂缓：无法读取 Claude 会话",
                "请在 Claude 桌面端打开该会话后立即重试",
            )
        } else if dsh_read_failed {
            (
                "交接已暂缓：无法读取 DSH 输出",
                "请确认 DeepSeek Harness 正在运行后立即重试",
            )
        } else {
            ("交接已暂缓：等待核验", "")
        };
        Banner {
            variant: Variant::Warn,
            title: title.into(),
            desc: hint.into(),
            code: Some(if demo_paused {
                "TARGET_DOCUMENT_UNAVAILABLE: open".to_owned()
            } else {
                error_code(detail)
            }),
            buttons: vec![view_log_button(), poll_button()],
        }
    } else if demo.is_none() && !available {
        Banner {
            variant: Variant::Warn,
            title: "监听正在启动或已中断".into(),
            desc: "未确认前不会发送。".into(),
            code: None,
            buttons: vec![view_log_button()],
        }
    } else if demo == Some("success")
        || (s.live["last_delivery"]["state"] == "sent"
            && s.live["last_delivery"]["at_ms"]
                .as_u64()
                .is_some_and(|t| now.saturating_sub(t) < NOTICE_MS))
    {
        let target =
            if demo == Some("success") || s.live["last_delivery"]["direction"] == "CLAUDE_TO_DSH" {
                "DSH"
            } else {
                "Claude"
            };
        Banner {
            variant: Variant::Success,
            title: format!("已发送给 {target}"),
            desc: format!("第 {} 轮已开始", s.dsh_turn.unwrap_or(0).max(1)),
            code: None,
            buttons: vec![],
        }
    } else if auto_on {
        if phase == "idle" {
            Banner {
                variant: Variant::Info,
                title: live_str(s, "status_text").to_owned(),
                desc: live_str(s, "detail").to_owned(),
                code: None,
                buttons: vec![poll_button()],
            }
        } else {
            // `waiting_dsh` covers both "DSH owes us a round" and "we handed the result
            // to Claude and Claude owes the answer". Deriving the wording here from the
            // phase name alone named the wrong side - the window said 等待 DSH 的回复
            // while Claude was the one working. The runtime already resolves which side
            // is actually being awaited, so its text is shown as-is rather than
            // recomputed. Duplicating that rule in two places is what let them disagree.
            Banner {
                variant: Variant::Info,
                title: live_str(s, "status_text").to_owned(),
                desc: live_str(s, "detail").to_owned(),
                code: None,
                buttons: vec![poll_button()],
            }
        }
    } else if matches!(phase.as_str(), "unclaimed" | "paused_by_user") || demo == Some("manual") {
        let (t, d) = if demo == Some("manual") {
            (
                "你已接管 Claude 对话".to_owned(),
                "自动交接暂停；讨论结束后，由你选择从哪条回复恢复。".to_owned(),
            )
        } else {
            (
                live_str(s, "status_text").to_owned(),
                live_str(s, "detail").to_owned(),
            )
        };
        Banner {
            variant: Variant::Neutral,
            title: t,
            desc: d,
            code: None,
            buttons: vec![poll_button()],
        }
    } else {
        Banner {
            variant: Variant::Neutral,
            title: "自动交接已关闭".into(),
            desc: "使用卡片里的按钮手动发送".into(),
            code: None,
            buttons: vec![poll_button()],
        }
    };
    // Overlay the transient notice on top of the state banner: variant and text come
    // from the notice, the action buttons stay so nothing vanishes mid-interaction.
    if let Some(n) = notice {
        banner.variant = n.variant;
        banner.title = n.title.clone();
        banner.desc = n.detail.clone();
        banner.code = None;
    }

    let interval = s.poll_seconds.max(1);
    let poll = if !listening {
        None
    } else if demo.is_some() {
        Some((interval * 6 / 10, 40))
    } else {
        let next = s.live["next_poll_at_ms"].as_u64().unwrap_or(0);
        if next == 0 {
            Some((0, 100))
        } else {
            let left = next.saturating_sub(now).div_ceil(1000).min(interval);
            let pct = 100 - (left * 100 / interval) as u32;
            Some((left, pct.min(100)))
        }
    };
    let demo_on = s.demo_enabled;
    Ui {
        available,
        listening,
        listener,
        auto_on,
        round: s.dsh_turn,
        claude_bound,
        claude_task: if claude_bound {
            s.claude_title.clone()
        } else {
            "–".into()
        },
        claude_status,
        dsh_task: s.dsh_title.clone(),
        dsh_sub,
        dsh_status,
        send_dsh: send_dsh || demo_on,
        send_claude: send_claude || demo_on,
        loading_dsh: sending && !to_claude,
        loading_claude: sending && to_claude,
        toggle_listen_enabled: demo_on || (available && needs_restore && !sending && !pending),
        toggle_auto_enabled: demo_on
            || (available
                && listening
                && !(sending && s.live["pending"]["stage"] == "draft_ready")),
        banner,
        poll,
    }
}

fn icon_btn_w(m: &dyn Measure, icon: bool, label: &str, f: Font) -> i32 {
    26 + if icon { 22 } else { 0 } + m.text_w(label, f)
}

pub fn build(
    m: &dyn Measure,
    width: i32,
    s: &Snapshot,
    watch_alive: bool,
    notice: Option<&Notice>,
    poll_input: u64,
    now: u64,
) -> View {
    let width = width.max(MIN_WIDTH);
    let ui = derive(s, watch_alive, notice, now);
    let mut v = View::default();
    let gw = width - 40;

    // ---- header (y 20..56) ----
    let cy = 38;
    v.rect(Box2::new(20, 22, 32, 32), 8, RAISED, Some(LINE));
    v.icon(Box2::new(27, 29, 18, 18), Icon::Swap, TEXT);
    let tw = m.text_w("A2AHandoff", Font::Title);
    v.text(
        Box2::new(64, cy - 10, tw, 18),
        "A2AHandoff",
        Font::Title,
        TEXT,
        Align::Left,
    );
    let sub = "智能体任务交接";
    let sw = m.text_w(sub, Font::Small);
    let sx = 64 + tw + 8;
    v.text(
        Box2::new(sx, 30, sw, 17),
        sub,
        Font::Small,
        TEXT3,
        Align::Left,
    );
    let pill = if ui.listening {
        "V1 运行中"
    } else {
        "V1 已停止监听"
    };
    let pw = 16 + 6 + 6 + m.text_w(pill, Font::Small);
    let px = sx + sw + 12;
    v.rect(
        Box2::new(px, 27, pw, 22),
        11,
        if ui.listening { OK_TINT } else { RAISED },
        None,
    );
    v.rect(
        Box2::new(px + 8, 35, 6, 6),
        3,
        if ui.listening { OK } else { TEXT3 },
        None,
    );
    v.text(
        Box2::new(px + 20, 29, pw - 28, 18),
        pill,
        Font::Small,
        if ui.listening { OK_TEXT } else { TEXT2 },
        Align::Left,
    );
    // The listener's own state, spelled out. The pill above answers "is the runtime
    // up"; this answers "is it actually watching, and what is it waiting for", which
    // is what a stuck handoff usually needs.
    v.text(
        Box2::new(px + pw + 10, 30, 220, 17),
        ui.listener.label(),
        Font::Small,
        if ui.listening { TEXT3 } else { WARN_TEXT },
        Align::Left,
    );
    let logs_w = icon_btn_w(m, true, "诊断日志", Font::Body);
    let bind_w = icon_btn_w(m, true, "绑定配置", Font::Body);
    let logs_x = width - 20 - logs_w;
    let bind_x = logs_x - 12 - bind_w;
    let sw1 = m.text_w("监听", Font::Body) + 8 + 32;
    let sw2 = m.text_w("自动交接", Font::Body) + 8 + 32;
    let gw_w = 26 + sw1 + 14 + 1 + 14 + sw2;
    let gx = bind_x - 12 - gw_w;
    let set_w = icon_btn_w(m, true, "设置", Font::Body);
    let set_x = gx - 12 - set_w;
    v.rect(Box2::new(gx, 22, gw_w, 32), 8, PANEL, Some(LINE));
    v.rect(Box2::new(gx + 13 + sw1 + 14, 30, 1, 16), 0, LINE, None);
    v.ctl(
        ID_RESTORE,
        Box2::new(gx + 13, 23, sw1, 30),
        "监听",
        Kind::Switch(ui.listening),
        ui.toggle_listen_enabled,
        None,
        false,
    );
    v.ctl(
        ID_TOGGLE,
        Box2::new(gx + 13 + sw1 + 29, 23, sw2, 30),
        "自动交接",
        Kind::Switch(ui.auto_on),
        ui.toggle_auto_enabled,
        None,
        false,
    );
    v.ctl(
        ID_BINDINGS,
        Box2::new(bind_x, 22, bind_w, 32),
        "绑定配置",
        Kind::Secondary,
        true,
        Some(Icon::Link),
        false,
    );
    v.ctl(
        ID_LOGS,
        Box2::new(logs_x, 22, logs_w, 32),
        "诊断日志",
        Kind::Secondary,
        true,
        Some(Icon::Terminal),
        false,
    );
    // Pushed last so the tab order reads 监听 → 自动交接 → 绑定配置 → 诊断日志 → 设置.
    v.ctl(
        ID_SETTINGS,
        Box2::new(set_x, 22, set_w, 32),
        "设置",
        Kind::Secondary,
        true,
        Some(Icon::Doc),
        false,
    );

    // ---- flow row (y 72..304) ----
    let cw = (gw - 72) / 2;
    let lx = 20;
    let rx = 20 + cw + 72;
    let rw = gw - 72 - cw;
    for (i, (x, w)) in [(lx, cw), (rx, rw)].into_iter().enumerate() {
        let claude = i == 0;
        v.rect(Box2::new(x, 72, w, 232), 12, PANEL, Some(LINE));
        let x0 = x + 17;
        let iw = w - 34;
        v.rect(
            Box2::new(x0, 91, 36, 36),
            10,
            if claude { CLAUDE_TINT } else { DSH_TINT },
            None,
        );
        v.text(
            Box2::new(x0, 91, 36, 36),
            if claude { "C" } else { "D" },
            Font::Avatar,
            if claude { CLAUDE_SOFT } else { DSH_SOFT },
            Align::Center,
        );
        // badge (right aligned)
        let (badge, bw) = if claude {
            let t = if ui.claude_bound {
                "已绑定"
            } else {
                "未绑定"
            };
            (
                t,
                2 + 16 + if ui.claude_bound { 16 } else { 0 } + m.text_w(t, Font::Small),
            )
        } else {
            let t = ui.listener.short();
            (t, 16 + 12 + m.text_w(t, Font::Small))
        };
        let bx = x0 + iw - bw;
        if claude {
            v.rect(Box2::new(bx, 98, bw, 22), 6, RAISED, Some(LINE));
            if ui.claude_bound {
                v.icon(Box2::new(bx + 9, 103, 12, 12), Icon::Check, TEXT2);
                v.text(
                    Box2::new(bx + 25, 100, bw - 33, 18),
                    badge,
                    Font::Small,
                    TEXT2,
                    Align::Left,
                );
            } else {
                v.text(
                    Box2::new(bx + 9, 100, bw - 18, 18),
                    badge,
                    Font::Small,
                    TEXT3,
                    Align::Left,
                );
            }
        } else {
            v.rect(
                Box2::new(bx, 98, bw, 22),
                6,
                if ui.listening { OK_TINT } else { RAISED },
                None,
            );
            v.rect(
                Box2::new(bx + 8, 106, 6, 6),
                3,
                if ui.listening { OK } else { TEXT3 },
                None,
            );
            v.text(
                Box2::new(bx + 20, 100, bw - 28, 18),
                badge,
                Font::Small,
                if ui.listening { OK_TEXT } else { TEXT3 },
                Align::Left,
            );
        }
        let name_w = (bx - 12 - (x0 + 48)).max(0);
        v.text(
            Box2::new(x0 + 48, 88, name_w, 21),
            if claude { "Claude" } else { "DeepSeek Harness" },
            Font::Name,
            TEXT,
            Align::Left,
        );
        v.text(
            Box2::new(x0 + 48, 111, name_w, 17),
            if claude {
                "窗口：Claude 桌面端".to_owned()
            } else {
                ui.dsh_sub.clone()
            },
            Font::Small,
            TEXT3,
            Align::Left,
        );
        v.text(
            Box2::new(x0, 142, iw, 17),
            "当前任务",
            Font::Small,
            TEXT3,
            Align::Left,
        );
        let task = if claude {
            &ui.claude_task
        } else {
            &ui.dsh_task
        };
        v.text(
            Box2::new(x0, 163, iw, 20),
            task.clone(),
            Font::Task,
            TEXT,
            Align::Left,
        );
        v.tips.push((Box2::new(x0, 164, iw, 20), task.clone()));
        let (tone, msg) = if claude {
            &ui.claude_status
        } else {
            &ui.dsh_status
        };
        let (dot, ink) = match tone {
            Tone::Ok => (OK, OK_TEXT),
            Tone::Warn => (WARN, WARN_TEXT),
            Tone::Muted => (TEXT3, TEXT2),
        };
        v.rect(Box2::new(x0, 203, 6, 6), 3, dot, None);
        v.text(
            Box2::new(x0 + 14, 196, iw - 14, 17),
            msg.clone(),
            Font::Small,
            ink,
            Align::Left,
        );
        let (id, label, kind, on, loading) = if claude {
            (
                ID_SEND_CLAUDE,
                "发送给 Claude",
                Kind::PrimaryClaude,
                ui.send_claude,
                ui.loading_claude,
            )
        } else {
            (
                ID_SEND_DSH,
                "发送给 DSH",
                Kind::PrimaryDsh,
                ui.send_dsh,
                ui.loading_dsh,
            )
        };
        v.ctl(
            id,
            Box2::new(x0, 251, iw, 36),
            if loading { "发送中…" } else { label },
            kind,
            on,
            Some(Icon::Send),
            loading,
        );
    }
    let mid = lx + cw + 36;
    v.rect(Box2::new(mid, 95, 1, 44), 0, LINE, None);
    v.text(
        Box2::new(mid - 30, 149, 60, 28),
        ui.round.map_or("–".to_owned(), |n| n.to_string()),
        Font::Round,
        TEXT,
        Align::Center,
    );
    v.text(
        Box2::new(mid - 30, 179, 60, 17),
        "轮",
        Font::Small,
        TEXT3,
        Align::Center,
    );
    v.icon(Box2::new(mid - 10, 206, 20, 20), Icon::Swap, TEXT3);
    v.rect(Box2::new(mid, 236, 1, 44), 0, LINE, None);

    // ---- banner (y 320..384) ----
    let b = &ui.banner;
    let (bg, border, ink, icon) = b.variant.look();
    v.rect(Box2::new(20, 320, gw, 64), 10, bg, Some(border));
    v.icon(Box2::new(37, 342, 20, 20), icon, ink);
    let mut xr = width - 37;
    let mut bl = xr;
    let mut placed: Vec<(&BannerButton, i32, i32)> = Vec::new();
    for bb in b.buttons.iter().rev() {
        let w = if bb.ghost {
            24 + m.text_w(bb.label, Font::Body)
        } else {
            icon_btn_w(m, bb.icon.is_some(), bb.label, Font::Body)
        };
        xr -= w;
        placed.push((bb, xr, w));
        bl = xr;
        xr -= 12;
    }
    let col_x = 69;
    let col_w = ((if b.buttons.is_empty() {
        width - 37
    } else {
        bl - 12
    }) - col_x)
        .max(0);
    let desc_h = if b.code.is_some() { 19 } else { 17 };
    let top = 320 + (64 - (20 + 4 + desc_h)) / 2;
    v.text(
        Box2::new(col_x, top, col_w, 20),
        b.title.clone(),
        Font::BannerTitle,
        TEXT,
        Align::Left,
    );
    let mut dx = col_x;
    if !b.desc.is_empty() {
        let dw = m.text_w(&b.desc, Font::Small).min(col_w);
        v.text(
            Box2::new(col_x, top + 24 + (desc_h - 17) / 2, dw, 17),
            b.desc.clone(),
            Font::Small,
            TEXT2,
            Align::Left,
        );
        dx += dw + 8;
    }
    if let Some(code) = &b.code {
        let room = (col_x + col_w - dx).max(0);
        let cw2 = (m.text_w(code, Font::Code) + 14).min(room);
        v.rect(Box2::new(dx, top + 24, cw2, 19), 4, RAISED, Some(LINE));
        v.text(
            Box2::new(dx + 7, top + 26, (cw2 - 14).max(0), 15),
            code.clone(),
            Font::Code,
            WARN_TEXT,
            Align::Left,
        );
        v.tips
            .push((Box2::new(dx, top + 24, cw2, 19), code.clone()));
    }
    for (bb, x, w) in placed.into_iter().rev() {
        v.ctl(
            bb.id,
            Box2::new(x, 336, w, 32),
            bb.label,
            if bb.ghost {
                Kind::Ghost
            } else {
                Kind::BannerSecondary
            },
            bb.enabled || s.demo_enabled,
            bb.icon,
            false,
        );
    }

    // ---- footer (y 400..452) ----
    v.rect(Box2::new(20, 400, gw, 52), 10, PANEL, Some(LINE));
    let fy = 426;
    let mut x = 37;
    let lw = m.text_w("自动轮询", Font::Label);
    v.text(
        Box2::new(x, fy - 10, lw, 18),
        "自动轮询",
        Font::Label,
        TEXT,
        Align::Left,
    );
    x += lw + 16;
    v.rect(Box2::new(x, 410, 106, 32), 8, RAISED, Some(LINE));
    v.rect(Box2::new(x + 33, 411, 40, 30), 0, PANEL, None);
    v.rect(Box2::new(x + 33, 411, 1, 30), 0, LINE, None);
    v.rect(Box2::new(x + 72, 411, 1, 30), 0, LINE, None);
    v.ctl(
        ID_MINUS,
        Box2::new(x + 1, 411, 32, 30),
        "−",
        Kind::Step,
        poll_input > MIN_POLL,
        None,
        false,
    );
    v.ctl(
        ID_VALUE,
        Box2::new(x + 34, fy - 9, 38, 18),
        &poll_input.to_string(),
        Kind::Edit,
        true,
        None,
        false,
    );
    v.ctl(
        ID_PLUS,
        Box2::new(x + 73, 411, 32, 30),
        "+",
        Kind::Step,
        poll_input < MAX_POLL,
        None,
        false,
    );
    x += 106 + 8;
    let mw = m.text_w("分钟", Font::Body);
    v.text(
        Box2::new(x, fy - 10, mw, 18),
        "分钟",
        Font::Body,
        TEXT2,
        Align::Left,
    );
    x += mw + 16;
    v.rect(Box2::new(x, 416, 1, 20), 0, LINE, None);
    x += 17;
    v.text(
        Box2::new(x, 412, 84, 17),
        "下次轮询",
        Font::Small,
        TEXT2,
        Align::Left,
    );
    let (left_txt, pct) = match ui.poll {
        Some((0, 100)) => ("即将检查".to_owned(), 100),
        Some((l, p)) => (format!("{l} 秒后"), p),
        None => ("已暂停".to_owned(), 0),
    };
    v.text(
        Box2::new(x + 84, 412, 84, 17),
        left_txt,
        Font::Small,
        if ui.poll.is_some() { TEXT } else { TEXT3 },
        Align::Right,
    );
    v.rect(Box2::new(x, 435, 168, 4), 2, LINE, None);
    let fw = 168 * pct as i32 / 100;
    if fw > 0 {
        v.rect(
            Box2::new(x, 435, fw.max(4), 4),
            2,
            if ui.poll.is_some() { DSH_ACCENT } else { TEXT3 },
            None,
        );
    }
    let save_w = icon_btn_w(m, false, "保存间隔", Font::Body);
    let tpl_w = icon_btn_w(m, true, "交接文案", Font::Body);
    let save_x = width - 33 - save_w;
    let tpl_x = save_x - 16 - tpl_w;
    v.ctl(
        ID_TEMPLATES,
        Box2::new(tpl_x, 410, tpl_w, 32),
        "交接文案",
        Kind::Secondary,
        s.demo.is_none() || s.demo_enabled,
        Some(Icon::Doc),
        false,
    );
    v.ctl(
        ID_SAVE,
        Box2::new(save_x, 410, save_w, 32),
        "保存间隔",
        Kind::Secondary,
        s.config_available,
        None,
        false,
    );
    v
}

pub fn px(dip: i32, dpi: u32) -> i32 {
    ((dip as i64 * dpi as i64 + 48) / 96) as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    /// Rough width model: CJK/full-width = 1em, ASCII = 0.55em.
    struct Est;
    impl Measure for Est {
        fn text_w(&self, s: &str, f: Font) -> i32 {
            let em = f.size() as f32;
            s.chars()
                .map(|c| if c.is_ascii() { em * 0.55 } else { em })
                .sum::<f32>()
                .ceil() as i32
        }
    }
    fn overlaps(a: Box2, b: Box2) -> bool {
        a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y
    }
    fn live(extra: serde_json::Value) -> Snapshot {
        let mut s = Snapshot::for_demo("ready");
        s.demo = None;
        s.runtime_age = Some(1);
        let mut l = json!({"mode":"live","enabled":true,"phase":"waiting_dsh","sending":false,
            "pending":null,"claude_ok":true,"dsh_ok":true,"status_text":"x","detail":"","next_poll_at_ms":0});
        for (k, v) in extra.as_object().unwrap() {
            l[k] = v.clone();
            // `derive` reads some values from the typed fields rather than from `live`,
            // so an overlay that only touched `live` would silently leave the demo
            // values in place and the test would exercise something else entirely.
            match k.as_str() {
                "dsh_turn" => s.dsh_turn = v.as_u64(),
                "reply_turn" => s.reply_turn = v.as_u64(),
                "dsh_running" => s.dsh_running = v.as_bool(),
                _ => {}
            }
        }
        s.live = l;
        s
    }
    fn banner(s: &Snapshot) -> Banner {
        derive(s, true, None, 1_000).banner
    }
    #[test]
    fn px_scaling() {
        assert_eq!(px(18, 192), 36);
        assert_eq!(px(36, 144), 54);
    }
    #[test]
    fn path_parser_handles_compact_arcs_and_close() {
        let p = flatten_path("M10 13a5 5 0 0 0 7.5.5l3-3");
        assert_eq!(p.len(), 1);
        let last = *p[0].last().unwrap();
        assert!(
            (last.0 - 20.5).abs() < 0.01 && (last.1 - 10.5).abs() < 0.01,
            "{last:?}"
        );
        let z = flatten_path("m22 2-7 20-4-9-9-4Z");
        assert_eq!(z[0].first(), z[0].last());
        for i in [
            Icon::Swap,
            Icon::Link,
            Icon::Warn,
            Icon::Refresh,
            Icon::Doc,
            Icon::CircleX,
        ] {
            assert!(!i.strokes().is_empty());
        }
    }
    #[test]
    fn stack_heights_match_spec() {
        // 20+36+16+232+16+64+16+52 = 452
        let v = build(&Est, 960, &Snapshot::for_demo("running"), true, None, 1, 0);
        let bottom = v
            .prims
            .iter()
            .filter_map(|p| match p {
                Prim::Rect { a, .. } => Some(a.y + a.h),
                _ => None,
            })
            .max()
            .unwrap();
        assert_eq!(bottom, 452);
    }
    #[test]
    fn controls_fit_and_never_overlap_at_all_widths_and_states() {
        for w in [960, 1100, 1400] {
            for st in [
                "running",
                "paused",
                "stopped",
                "success",
                "error",
                "countdown",
                "manual",
                "busy",
            ] {
                let v = build(&Est, w, &Snapshot::for_demo(st), true, None, 10, 0);
                for (i, a) in v.controls.iter().enumerate() {
                    assert!(a.area.x >= 0 && a.area.x + a.area.w <= w, "{st} {w} {a:?}");
                    assert!(a.area.y + a.area.h <= 452);
                    for b in v.controls.iter().skip(i + 1) {
                        assert!(!overlaps(a.area, b.area), "{st} {w}: {} vs {}", a.id, b.id);
                    }
                }
            }
        }
    }
    #[test]
    fn tab_order_follows_spec() {
        // The banner slot holds different buttons per state, so the spec order is
        // asserted for both states that have one. 「重新核验」 is gone; the warn
        // banner now carries 「立即轮询」.
        let cases: [(&str, Vec<usize>); 2] = [
            ("paused", vec![ID_BANNER_LOGS, ID_BANNER_POLL]),
            ("running", vec![ID_BANNER_POLL]),
        ];
        for (state, banner_btns) in cases {
            let v = build(&Est, 960, &Snapshot::for_demo(state), true, None, 1, 0);
            let ids: Vec<usize> = v.controls.iter().map(|c| c.id).collect();
            let mut want = vec![
                ID_RESTORE,
                ID_TOGGLE,
                ID_BINDINGS,
                ID_LOGS,
                ID_SETTINGS,
                ID_SEND_CLAUDE,
                ID_SEND_DSH,
            ];
            want.extend(banner_btns);
            want.extend([ID_MINUS, ID_VALUE, ID_PLUS, ID_TEMPLATES, ID_SAVE]);
            assert_eq!(ids, want, "tab order for demo={state}");
        }
    }
    /// 「重新核验」 must be gone: no control may carry the removed id, in any state.
    #[test]
    fn reverify_control_is_gone_from_every_state() {
        const REMOVED_ID: usize = 307;
        for st in [
            "running",
            "paused",
            "stopped",
            "success",
            "error",
            "countdown",
            "manual",
            "busy",
            "cancelled",
            "longtext",
        ] {
            let v = build(&Est, 960, &Snapshot::for_demo(st), true, None, 1, 0);
            assert!(
                !v.controls.iter().any(|c| c.id == REMOVED_ID),
                "demo={st} still builds a control with the removed re-verify id"
            );
            assert!(
                !v.controls.iter().any(|c| c.label.contains("重新核验")),
                "demo={st} still labels a control 重新核验"
            );
        }
        // The banner's hint text must not point at a button that no longer exists.
        let mut s = Snapshot::for_demo("paused");
        s.demo = None;
        s.live = serde_json::json!({"mode":"live","phase":"waiting_dsh","claude_ok":false,
            "dsh_ok":true,"detail":"读取失败：TARGET_DOCUMENT_UNAVAILABLE: open"});
        let ui = derive(&s, true, None, 0);
        assert!(ui.banner.variant == Variant::Warn);
        assert!(!ui.banner.desc.contains("重新核验"), "{}", ui.banner.desc);
    }
    /// The listener must report one of the seven product states, derived from what the
    /// runtime publishes - never a hardcoded label that can contradict the banner.
    #[test]
    fn listener_state_matches_what_the_runtime_is_doing() {
        // Nothing bound yet.
        assert_eq!(
            derive(&live(json!({"phase": "unclaimed"})), true, None, 1_000).listener,
            ListenerState::SessionUnavailable
        );
        // The runtime is reading a different session than the one bound.
        let mut s = live(json!({"phase": "waiting_dsh"}));
        s.watch_session_matches = false;
        assert_eq!(
            derive(&s, true, None, 1_000).listener,
            ListenerState::SessionMismatch
        );
        // Heartbeat stale, and the runtime process gone.
        let mut s = live(json!({"phase": "waiting_dsh"}));
        s.runtime_age = Some(999);
        assert_eq!(
            derive(&s, true, None, 1_000).listener,
            ListenerState::HeartbeatExpired
        );
        assert_eq!(
            derive(&live(json!({"phase": "waiting_dsh"})), false, None, 1_000).listener,
            ListenerState::HeartbeatExpired
        );
        // Attached to a round still in flight.
        assert_eq!(
            derive(
                &live(json!({"phase": "waiting_dsh", "dsh_turn": 21, "reply_turn": 20})),
                true,
                None,
                1_000
            )
            .listener,
            ListenerState::TurnConnected
        );
        // Caught up: the last completed round has been accounted for.
        let caught = live(json!({"phase": "waiting_dsh", "dsh_turn": 21, "reply_turn": 21}));
        let caught_ui = derive(&caught, true, None, 1_000);
        assert_eq!(
            caught_ui.listener,
            ListenerState::WaitingNextTurn,
            "dsh_turn={:?} reply_turn={:?}",
            caught.dsh_turn,
            caught.reply_turn
        );
        assert_eq!(
            derive(
                &live(json!({"phase": "waiting_dsh", "dsh_turn": 21, "reply_turn": 21})),
                true,
                None,
                1_000
            )
            .listener,
            ListenerState::WaitingNextTurn
        );
        // Watching and idle.
        assert_eq!(
            derive(&live(json!({"phase": "waiting_claude"})), true, None, 1_000).listener,
            ListenerState::Normal
        );
        // `waiting_dsh` after handing the result to Claude: the next move is Claude's,
        // so naming it 等待下一轮 (i.e. waiting on DSH) would point at the wrong side.
        assert_eq!(
            derive(
                &live(json!({
                    "phase": "waiting_dsh",
                    "dsh_turn": 21,
                    "reply_turn": 21,
                    "last_delivery": {"direction": "DSH_TO_CLAUDE", "state": "sent"}
                })),
                true,
                None,
                1_000
            )
            .listener,
            ListenerState::Normal
        );
        // Still waiting for DSH to produce the next round.
        assert_eq!(
            derive(
                &live(json!({
                    "phase": "waiting_dsh",
                    "dsh_turn": 21,
                    "reply_turn": 21,
                    "last_delivery": {"direction": "CLAUDE_TO_DSH", "state": "sent"}
                })),
                true,
                None,
                1_000
            )
            .listener,
            ListenerState::WaitingNextTurn
        );
        // Observation continues while automatic sends are paused.
        assert_eq!(
            derive(&live(json!({"phase": "paused_by_user"})), true, None, 1_000).listener,
            ListenerState::Paused
        );
        // Every state has a distinct label, so the window can never show two names for
        // one condition or one name for two conditions.
        let all = [
            ListenerState::SessionUnavailable,
            ListenerState::SessionMismatch,
            ListenerState::HeartbeatExpired,
            ListenerState::TurnConnected,
            ListenerState::WaitingNextTurn,
            ListenerState::Normal,
            ListenerState::Paused,
        ];
        let mut labels: Vec<&str> = all.iter().map(|x| x.label()).collect();
        labels.sort_unstable();
        let count = labels.len();
        labels.dedup();
        assert_eq!(labels.len(), count, "listener labels must be unique");
    }
    /// Exhaustive: every phase the runtime can publish maps to a named listener state.
    ///
    /// `listener` is derived from `phase`, so a phase with no branch would silently fall
    /// through to a default and the window would describe something the runtime never
    /// said. This pins the complete set, which is the ones `publish()` writes.
    #[test]
    fn every_runtime_phase_has_a_listener_state() {
        let cases = [
            ("unclaimed", ListenerState::SessionUnavailable),
            ("waiting_dsh", ListenerState::WaitingNextTurn),
            ("waiting_claude", ListenerState::Normal),
            ("idle", ListenerState::Normal),
            ("paused_by_user", ListenerState::Paused),
            ("hold_preparation", ListenerState::Paused),
            ("hold_send_uncertain", ListenerState::WaitingNextTurn),
        ];
        for (phase, want) in cases {
            let ui = derive(
                &live(json!({"phase": phase, "dsh_turn": 21, "reply_turn": 21})),
                true,
                None,
                1_000,
            );
            assert_eq!(ui.listener, want, "phase {phase}");
            // Every state must render a non-empty label, or the window shows a gap where
            // the listener status belongs.
            assert!(!ui.listener.label().is_empty(), "label for {phase}");
            assert!(!ui.listener.short().is_empty(), "badge for {phase}");
        }
        // The listener and the automatic-send flag are separate axes: observation keeps
        // running in every phase above except "nothing bound yet".
        for phase in ["waiting_dsh", "waiting_claude", "idle", "paused_by_user"] {
            let ui = derive(&live(json!({"phase": phase})), true, None, 1_000);
            assert!(ui.listening, "observation continues in {phase}");
        }
    }
    /// 「监听」 is the one-way 「恢复监听」 action: it must be disabled whenever the
    /// listener is already running, and only offered where there is something to
    /// restore. Otherwise it reads as a switch that is on but cannot be turned off.
    #[test]
    fn listen_switch_is_only_offered_when_there_is_something_to_restore() {
        // Bound and being watched -> read-only state display.
        for phase in [
            "waiting_dsh",
            "waiting_claude",
            "idle",
            "paused_by_user",
            "hold_send_uncertain",
            "hold_preparation",
        ] {
            let ui = derive(&live(json!({"phase": phase})), true, None, 1_000);
            assert!(
                ui.listening,
                "phase {phase} is still being watched even when automatic sends stop"
            );
        }
        // Only a phase with nothing bound is genuinely not listening.
        for phase in ["unclaimed", ""] {
            let ui = derive(&live(json!({"phase": phase})), true, None, 1_000);
            assert!(!ui.listening, "phase {phase} is not listening");
        }
        // Already listening -> no 恢复监听 control.
        for phase in ["waiting_dsh", "waiting_claude", "idle"] {
            let ui = derive(&live(json!({"phase": phase})), true, None, 1_000);
            assert!(
                !ui.toggle_listen_enabled,
                "phase {phase} must not offer 恢复监听"
            );
        }
        // Paused / holding -> offered, because a baseline has to be re-taken.
        for phase in ["unclaimed", "paused_by_user", "hold_send_uncertain"] {
            let ui = derive(&live(json!({"phase": phase})), true, None, 1_000);
            assert!(
                ui.toggle_listen_enabled,
                "phase {phase} must offer 恢复监听"
            );
        }
        // Never offer it in the middle of a handoff.
        for extra in [
            json!({"sending": true}),
            json!({"pending": {"stage": "draft_ready"}}),
        ] {
            let mut l = json!({"phase": "unclaimed", "mode": "live", "enabled": true,
                "sending": false, "pending": null, "claude_ok": true, "dsh_ok": true,
                "status_text": "x", "detail": "", "next_poll_at_ms": 0});
            for (k, v) in extra.as_object().unwrap() {
                l[k] = v.clone();
            }
            let mut s = Snapshot::for_demo("ready");
            s.demo = None;
            s.runtime_age = Some(1);
            s.live = l;
            let ui = derive(&s, true, None, 1_000);
            assert!(
                !ui.toggle_listen_enabled,
                "must not offer 恢复监听 while busy: {extra}"
            );
        }
    }
    #[test]
    fn stepper_limits_disable_buttons() {
        let lo = build(
            &Est,
            960,
            &Snapshot::for_demo("running"),
            true,
            None,
            MIN_POLL,
            0,
        );
        assert!(
            !lo.controls
                .iter()
                .find(|c| c.id == ID_MINUS)
                .unwrap()
                .enabled
        );
        let hi = build(
            &Est,
            960,
            &Snapshot::for_demo("running"),
            true,
            None,
            MAX_POLL,
            0,
        );
        assert!(
            !hi.controls
                .iter()
                .find(|c| c.id == ID_PLUS)
                .unwrap()
                .enabled
        );
    }
    #[test]
    fn demo_never_enables_send() {
        for st in ["running", "paused", "countdown", "manual", "busy"] {
            let v = build(&Est, 960, &Snapshot::for_demo(st), true, None, 1, 0);
            for c in v
                .controls
                .iter()
                .filter(|c| [ID_SEND_DSH, ID_SEND_CLAUDE].contains(&c.id))
            {
                assert!(!c.enabled, "{st}");
            }
        }
    }
    #[test]
    fn paused_error_banner_matches_reference_state_a() {
        let b = derive(&Snapshot::for_demo("paused"), true, None, 0).banner;
        assert_eq!(b.variant, Variant::Warn);
        assert_eq!(b.title, "交接已暂缓：无法读取 Claude 会话");
        assert_eq!(b.code.as_deref(), Some("TARGET_DOCUMENT_UNAVAILABLE: open"));
        assert_eq!(b.buttons.len(), 2);
    }
    #[test]
    fn running_banner_shows_the_runtime_text_rather_than_recomputing_it() {
        // The waiting phases no longer compose their own wording from the phase name:
        // doing that named the wrong side when `waiting_dsh` actually meant "we handed
        // the result to Claude and Claude owes the answer". The banner now shows
        // whatever the runtime resolved, so a fixture without a live payload yields an
        // empty banner rather than a fabricated sentence.
        let b = derive(&Snapshot::for_demo("running"), true, None, 0).banner;
        assert_eq!(b.variant, Variant::Info);
        assert_eq!(b.title, "");
        assert_eq!(b.desc, "");

        // With the runtime's text present it is passed through verbatim.
        let s = live(json!({
            "auto": true,
            "status_text": "等待 Claude 回复",
            "detail": "第 21 轮结果已投递；Claude 回复后转交 DSH。"
        }));
        let mut s = s;
        s.demo = None;
        let b = banner(&s);
        assert_eq!(b.title, "等待 Claude 回复");
        assert_eq!(b.desc, "第 21 轮结果已投递；Claude 回复后转交 DSH。");
    }
    #[test]
    fn live_claude_read_failure_is_warn_with_raw_code() {
        let s = live(json!({"claude_ok":false,"status_text":"交接暂缓，等待核验",
            "detail":"Claude 读取失败：TARGET_DOCUMENT_UNAVAILABLE: open the bound Claude conversation."}));
        let b = banner(&s);
        assert_eq!(b.variant, Variant::Warn);
        assert_eq!(
            b.code.as_deref(),
            Some("TARGET_DOCUMENT_UNAVAILABLE: open the bound Claude conversation.")
        );
    }
    #[test]
    fn priority_error_over_warn_over_success_over_info() {
        let ok = json!({"at_ms":900,"state":"sent","direction":"CLAUDE_TO_DSH"});
        let s = live(json!({"last_delivery":ok}));
        assert_eq!(banner(&s).variant, Variant::Success);
        let s = live(json!({"last_delivery":ok,"claude_ok":false,"detail":"a：B"}));
        assert_eq!(banner(&s).variant, Variant::Warn);
        let s = live(
            json!({"last_delivery":ok,"claude_ok":false,"phase":"hold_send_uncertain","detail":"a：B"}),
        );
        assert_eq!(banner(&s).variant, Variant::Error);
        let old = json!({"at_ms":1,"state":"sent","direction":"CLAUDE_TO_DSH"});
        let s = live(json!({"last_delivery":old}));
        assert_eq!(derive(&s, true, None, 9_000).banner.variant, Variant::Info);
    }
    #[test]
    fn auto_off_gives_neutral_and_listen_off_disables_auto() {
        let s = live(json!({"enabled":false}));
        let ui = derive(&s, true, None, 0);
        assert!(ui.listening && !ui.auto_on);
        assert_eq!(ui.banner.variant, Variant::Neutral);
        let s = live(json!({"phase":"unclaimed"}));
        let ui = derive(&s, true, None, 0);
        assert!(!ui.listening && !ui.toggle_auto_enabled && ui.toggle_listen_enabled);
    }
    #[test]
    fn listen_switch_cannot_be_turned_off() {
        let ui = derive(&live(json!({})), true, None, 0);
        assert!(ui.listening && !ui.toggle_listen_enabled);
    }
    #[test]
    fn pending_shows_cancel_and_live_buttons_gate_like_before() {
        let s = live(
            json!({"pending":{"id":"x","direction":"DSH_TO_CLAUDE","stage":"draft_ready","deadline_ms":0},
            "status_text":"DSH → Claude · 5 秒","detail":"d"}),
        );
        let ui = derive(&s, true, None, 0);
        assert!(ui
            .banner
            .buttons
            .iter()
            .any(|b| b.id == ID_CANCEL && b.enabled));
        assert!(!ui.send_dsh && !ui.send_claude);
        let stale = derive(&s, false, None, 0);
        assert!(!stale.available);
    }
    #[test]
    fn sending_shows_spinner_on_target_button() {
        let s = live(
            json!({"sending":true,"pending":{"id":"x","direction":"DSH_TO_CLAUDE","stage":"queued","deadline_ms":0}}),
        );
        let v = build(&Est, 960, &s, true, None, 1, 0);
        let c = v.controls.iter().find(|c| c.id == ID_SEND_CLAUDE).unwrap();
        assert!(c.loading && c.label == "发送中…");
    }
    #[test]
    fn progress_and_countdown() {
        let s = live(json!({"next_poll_at_ms": 61_000}));
        let mut s = s;
        s.poll_seconds = 100;
        let ui = derive(&s, true, None, 1_000);
        assert_eq!(ui.poll, Some((60, 40)));
    }
    /// A transient notice replaces the banner TEXT but keeps the state's action
    /// buttons, so 「立即轮询」 etc. no longer vanish for the notice's 4 s lifetime.
    #[test]
    fn notice_replaces_banner_text_but_keeps_the_states_buttons() {
        let base = derive(&live(json!({})), true, None, 1_000).banner;
        let n = Notice {
            variant: Variant::Success,
            title: "t".into(),
            detail: "d".into(),
        };
        let with = derive(&live(json!({})), true, Some(&n), 1_000).banner;
        assert_eq!(with.variant, Variant::Success);
        assert_eq!(with.title, "t");
        assert_eq!(with.desc, "d");
        assert_eq!(with.code, None);
        assert_eq!(
            with.buttons.iter().map(|x| x.id).collect::<Vec<_>>(),
            base.buttons.iter().map(|x| x.id).collect::<Vec<_>>(),
            "the notice must not change which actions are offered"
        );
    }
    /// The state's buttons survive a notice; the notice only changes the wording.
    #[test]
    fn transient_notice_no_longer_hides_the_states_action_buttons() {
        let mut s = live(json!({}));
        s.live["enabled"] = json!(false);
        let base = derive(&s, true, None, 1_000).banner;
        assert_eq!(base.title, "自动交接已关闭");
        assert_eq!(
            base.buttons.iter().map(|x| x.id).collect::<Vec<_>>(),
            vec![ID_BANNER_POLL]
        );
        let n = Notice {
            variant: Variant::Info,
            title: "操作已提交".into(),
            detail: "运行器将核验会话后执行。".into(),
        };
        let with = derive(&s, true, Some(&n), 1_000).banner;
        assert_eq!(with.title, "操作已提交");
        assert_eq!(
            with.buttons.iter().map(|x| x.id).collect::<Vec<_>>(),
            vec![ID_BANNER_POLL],
            "立即轮询 must stay available while the notice is shown"
        );
    }
    /// A held delivery may be retried only when nothing could have reached the peer.
    ///
    /// `draft_unverified` means the adapter refused before submitting anything, so re-running
    /// it cannot duplicate a message. `submit_uncertain` may already be delivered, and
    /// re-sending that is exactly what the hold exists to prevent.
    #[test]
    fn only_an_unverified_draft_offers_a_retry() {
        let hold = |state: &str| {
            let mut s = Snapshot::for_demo("running");
            s.demo = None;
            s.live = json!({"mode":"live","phase":"hold_send_uncertain","sending":false,
                "pending":null,"claude_ok":true,"dsh_ok":true,
                "status_text":"交接暂缓，等待核验",
                "detail":"本次已停止：DRAFT_WRITE_UNVERIFIED",
                "last_delivery":{"state":state,"direction":"DSH_TO_CLAUDE"},
                "next_poll_at_ms":0});
            derive(&s, true, None, 1_000).banner
        };
        let unverified = hold("draft_unverified");
        let ids = unverified.buttons.iter().map(|b| b.id).collect::<Vec<_>>();
        assert!(
            ids.contains(&ID_BANNER_RETRY),
            "an unverified draft must offer 重试本次投递, got {ids:?}"
        );
        assert!(
            ids.contains(&ID_BANNER_LOGS),
            "查看日志 stays available, got {ids:?}"
        );

        for state in ["submit_uncertain", "send_attempted", "sent", "uncertain"] {
            let ids = hold(state).buttons.iter().map(|b| b.id).collect::<Vec<_>>();
            assert!(
                !ids.contains(&ID_BANNER_RETRY),
                "{state} may already be at the peer and must not offer a retry, got {ids:?}"
            );
        }
    }
    /// The occupied-input hold must say what to do, not only which code came back.
    #[test]
    fn an_occupied_input_explains_itself() {
        let hold = |detail: &str| {
            let mut s = Snapshot::for_demo("running");
            s.demo = None;
            s.live = json!({"mode":"live","phase":"hold_send_uncertain","sending":false,
                "pending":null,"claude_ok":true,"dsh_ok":true,
                "status_text":"交接暂缓，等待核验","detail":detail,
                "last_delivery":{"state":"draft_unverified","direction":"DSH_TO_CLAUDE"},
                "next_poll_at_ms":0});
            derive(&s, true, None, 1_000).banner
        };
        for detail in [
            "本次已停止：DRAFT_OCCUPIED_PRESERVED",
            "本次已停止：DRAFT_OCCUPIED",
            "本次已停止：EXISTING_ATTACHMENT_PRESERVED",
        ] {
            let b = hold(detail);
            assert_eq!(
                b.code.as_deref(),
                Some(detail.trim_start_matches("本次已停止：")),
                "the code stays visible for {detail}"
            );
            assert!(
                b.desc.contains("清空后再点重试"),
                "{detail} must say how to clear the hold, got '{}'",
                b.desc
            );
        }
        // Every other hold keeps the bare code and adds nothing.
        let other = hold("本次已停止：DRAFT_WRITE_UNVERIFIED");
        assert!(other.desc.is_empty(), "got '{}'", other.desc);
    }
    /// A delivery held because the target input was occupied is still queued, so the same
    /// retry button must be offered there - that is the case with nothing else to press.
    #[test]
    fn a_queued_delivery_held_on_an_occupied_input_offers_a_retry() {
        let held = |phase: &str, pending: bool| {
            let mut s = Snapshot::for_demo("running");
            s.demo = None;
            s.live = json!({"mode":"live","phase":phase,"sending":false,
                "pending": if pending { json!({"id":"p","direction":"DSH_TO_CLAUDE","stage":"waiting_target","deadline_ms":0}) } else { json!(null) },
                "claude_ok":true,"dsh_ok":true,
                "status_text":"交接暂缓，等待核验",
                "detail":"目标输入框已有草稿，已保留；清空后点「重试本次投递」。",
                "last_delivery":{"state":"sent","direction":"DSH_TO_CLAUDE"},
                "next_poll_at_ms":0});
            derive(&s, true, None, 1_000).banner
        };
        let ids = held("hold_preparation", true)
            .buttons
            .iter()
            .map(|b| b.id)
            .collect::<Vec<_>>();
        assert!(
            ids.contains(&ID_BANNER_RETRY),
            "an occupied input with a queued delivery must offer a retry, got {ids:?}"
        );
        // Without a pending there is nothing to retry, so no button.
        let ids = held("hold_preparation", false)
            .buttons
            .iter()
            .map(|b| b.id)
            .collect::<Vec<_>>();
        assert!(
            !ids.contains(&ID_BANNER_RETRY),
            "nothing queued means nothing to retry, got {ids:?}"
        );
        // And an ordinary waiting phase offers the usual pair only.
        let ids = held("waiting_claude", true)
            .buttons
            .iter()
            .map(|b| b.id)
            .collect::<Vec<_>>();
        assert!(
            !ids.contains(&ID_BANNER_RETRY),
            "only a hold offers the retry, got {ids:?}"
        );
    }
    #[test]
    fn unbound_claude_card() {
        let mut s = Snapshot::for_demo("running");
        s.claude_title.clear();
        s.claude_session.clear();
        let ui = derive(&s, true, None, 0);
        assert_eq!(ui.claude_status.1, "尚未绑定窗口");
        assert_eq!(ui.claude_task, "–");
    }
    #[test]
    fn long_texts_stay_inside_their_boxes() {
        let mut s = Snapshot::for_demo("paused");
        s.dsh_title = "一二三四五六七八九十".repeat(20);
        let v = build(&Est, 960, &s, true, None, 1, 0);
        for p in &v.prims {
            if let Prim::Text { a, .. } = p {
                assert!(a.x >= 0 && a.x + a.w <= 960 && a.w >= 0);
            }
        }
    }
    /// T3: an over-long task name and an over-long error code must be clipped to the
    /// draw box (the painter sets DT_END_ELLIPSIS, win.rs) and must expose a tooltip
    /// carrying the full original string.
    #[test]
    fn overlong_task_and_error_code_are_clipped_and_keep_full_tooltips() {
        let task = "超长任务名".repeat(40);
        let code = format!(
            "SEND_UNCERTAIN: composer not confirmed {}",
            "retry-budget-exhausted;".repeat(6)
        );
        let mut s = Snapshot::for_demo("error");
        // demo=error paints a fixed reference code; drop the demo flag so `derive`
        // takes the live `hold_send_uncertain` branch and uses the raw detail string.
        s.demo = None;
        s.live = serde_json::json!({"mode":"live","phase":"hold_send_uncertain","sending":false,
            "pending":null,"claude_ok":true,"dsh_ok":true,
            "status_text":"发送给 DSH 未确认","detail":format!("发送失败：{code}"),
            "last_delivery":{"state":"uncertain","direction":"CLAUDE_TO_DSH"},
            "next_poll_at_ms":0});
        s.dsh_title = task.clone();
        s.claude_title = task.clone();
        let v = build(&Est, 960, &s, true, None, 1, 0);
        let est = Est;

        // 1. The task text never gets a box wider than the card's inner width.
        let task_prims: Vec<Box2> = v
            .prims
            .iter()
            .filter_map(|p| match p {
                Prim::Text { a, s: t, .. } if *t == task => Some(*a),
                _ => None,
            })
            .collect();
        assert_eq!(task_prims.len(), 2, "both cards show their task name");
        for a in task_prims {
            assert!(
                a.w < est.text_w(&task, Font::Task),
                "long task name must be clipped, not given an unbounded box: {a:?}"
            );
        }

        // 2. The error code chip is clipped, and the whole code stays in its tooltip.
        let code_box = v
            .prims
            .iter()
            .filter_map(|p| match p {
                Prim::Text { a, s: t, f, .. } if *f == Font::Code => Some((*a, t.clone())),
                _ => None,
            })
            .find(|(_, t)| t.starts_with("SEND_UNCERTAIN"))
            .expect("error banner paints the raw code");
        assert_eq!(code_box.1, code, "the painted code is the full raw string");
        assert!(
            code_box.0.w < est.text_w(&code, Font::Code),
            "long error code must be clipped: {:?}",
            code_box.0
        );
        assert!(
            code_box.0.x + code_box.0.w <= 960 - 20,
            "clipped code chip must stay inside the banner"
        );
        let tip = v
            .tips
            .iter()
            .find(|(_, t)| *t == code)
            .expect("full error code is exposed as a tooltip");
        assert!(
            tip.0.w >= code_box.0.w && tip.0.x + tip.0.w <= 960 - 20,
            "tooltip hot zone stays inside the banner: {:?}",
            tip.0
        );
    }
    /// T3: the `--demo=longtext` fixture really does overflow both the task-name line
    /// and the banner error-code chip, and it stays inside the window.
    #[test]
    fn longtext_demo_fixture_overflows_and_still_fits() {
        let v = build(&Est, 960, &Snapshot::for_demo("longtext"), true, None, 1, 0);
        let est = Est;
        let mut saw_long_task = false;
        let mut saw_long_code = false;
        for p in &v.prims {
            if let Prim::Text { a, s, f, .. } = p {
                assert!(a.x >= 0 && a.x + a.w <= 960, "{s:?} escapes the window");
                if *f == Font::Task && s.starts_with("超长任务名") {
                    assert!(a.w < est.text_w(s, *f), "task name is clipped");
                    saw_long_task = true;
                }
                if *f == Font::Code && s.starts_with("SEND_UNCERTAIN") {
                    assert!(a.w < est.text_w(s, *f), "error code is clipped");
                    saw_long_code = true;
                }
            }
        }
        assert!(
            saw_long_task && saw_long_code,
            "fixture must overflow both lines"
        );
        for (i, a) in v.controls.iter().enumerate() {
            assert!(a.area.x >= 0 && a.area.x + a.area.w <= 960 && a.area.y + a.area.h <= 452);
            for b in v.controls.iter().skip(i + 1) {
                assert!(!overlaps(a.area, b.area), "{} vs {}", a.id, b.id);
            }
        }
    }
    /// Regression guard for the empty-`status_text` fault: the runner can report
    /// `hold_send_uncertain` without any status text, which produced a banner title
    /// of "". `draw_text` used to hand that straight to DrawTextW and take an access
    /// violation in user32, which stopped the whole window from painting.
    #[test]
    fn hold_without_status_text_yields_empty_title_but_never_empty_text_prims() {
        let mut s = Snapshot::for_demo("running");
        s.demo = None;
        s.live = serde_json::json!({
            "mode": "live",
            "phase": "hold_send_uncertain",
            "sending": false,
            "pending": null,
            "claude_ok": true,
            "dsh_ok": true,
            "detail": "发送失败：SEND_UNCERTAIN: composer not confirmed"
        });
        let ui = derive(&s, true, None, 0);
        assert_eq!(ui.banner.variant, Variant::Error);
        assert_eq!(ui.banner.title, "", "runner sent no status text");
        assert!(!ui.banner.code.as_deref().unwrap_or("").is_empty());

        // The empty title still reaches the display list as an empty Text primitive;
        // `win::draw_text` is what skips it. Confirm it is the only empty one and that
        // the layout around it is still sane.
        let v = build(&Est, 960, &s, true, None, 1, 0);
        let empties: Vec<Box2> = v
            .prims
            .iter()
            .filter_map(|p| match p {
                Prim::Text { a, s: t, .. } if t.is_empty() => Some(*a),
                _ => None,
            })
            .collect();
        assert_eq!(
            empties.len(),
            1,
            "only the banner title is empty: {empties:?}"
        );
        for (i, a) in v.controls.iter().enumerate() {
            assert!(a.area.x >= 0 && a.area.x + a.area.w <= 960 && a.area.y + a.area.h <= 452);
            for b in v.controls.iter().skip(i + 1) {
                assert!(!overlaps(a.area, b.area), "{} vs {}", a.id, b.id);
            }
        }
    }
}
