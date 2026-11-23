//! Small shared UI pieces: icons, time formats, cards, timestamp chips.

use rune::tok::TextStyle;
use rune::{Color, El, Icon, col, el, icon, oklch, row, text};
use greffier_core::{Stage, Status};
use rune_kit::Kit;

use crate::App;

// Icons from Lucide (https://lucide.dev, ISC license): each body is the children of the Lucide
// SVG, on its 24-unit grid; the group carries the attributes Lucide puts on its `<svg>` element.
macro_rules! lucide {
    ($name:ident, $id:literal, $body:literal) => {
        pub static $name: Icon = Icon {
            name: concat!("lucide-", $id),
            view: 24.0,
            body: concat!(r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#, $body, "</g>"),
        };
    };
}

lucide!(PLAY, "play", r#"<path d="M5 5a2 2 0 0 1 3.008-1.728l11.997 6.998a2 2 0 0 1 .003 3.458l-12 7A2 2 0 0 1 5 19z"/>"#);
lucide!(PAUSE, "pause", r#"<rect x="14" y="3" width="5" height="18" rx="1"/><rect x="5" y="3" width="5" height="18" rx="1"/>"#);
lucide!(BACK, "rotate-ccw", r#"<path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/>"#);
lucide!(FORWARD, "rotate-cw", r#"<path d="M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8"/><path d="M21 3v5h-5"/>"#);
lucide!(EXPAND, "maximize", r#"<path d="M8 3H5a2 2 0 0 0-2 2v3"/><path d="M21 8V5a2 2 0 0 0-2-2h-3"/><path d="M3 16v3a2 2 0 0 0 2 2h3"/><path d="M16 21h3a2 2 0 0 0 2-2v-3"/>"#);
lucide!(SHRINK, "minimize", r#"<path d="M8 3v3a2 2 0 0 1-2 2H3"/><path d="M21 8h-3a2 2 0 0 1-2-2V3"/><path d="M3 16h3a2 2 0 0 1 2 2v3"/><path d="M16 21v-3a2 2 0 0 1 2-2h3"/>"#);
// Record and stop are filled, as on every recorder: Lucide's circle and square, filled.
lucide!(RECORD, "circle", r#"<circle cx="12" cy="12" r="10" fill="currentColor"/>"#);
lucide!(STOP, "square", r#"<rect width="18" height="18" x="3" y="3" rx="2" fill="currentColor"/>"#);
lucide!(HOME, "house", r#"<path d="M15 21v-8a1 1 0 0 0-1-1h-4a1 1 0 0 0-1 1v8"/><path d="M3 10a2 2 0 0 1 .709-1.528l7-6a2 2 0 0 1 2.582 0l7 6A2 2 0 0 1 21 10v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>"#);
lucide!(SLIDERS, "sliders-horizontal", r#"<path d="M10 5H3"/><path d="M12 19H3"/><path d="M14 3v4"/><path d="M16 17v4"/><path d="M21 12h-9"/><path d="M21 19h-5"/><path d="M21 5h-7"/><path d="M8 10v4"/><path d="M8 12H3"/>"#);
lucide!(SOUND, "volume-2", r#"<path d="M11 4.702a.705.705 0 0 0-1.203-.498L6.413 7.587A1.4 1.4 0 0 1 5.416 8H3a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2.416a1.4 1.4 0 0 1 .997.413l3.383 3.384A.705.705 0 0 0 11 19.298z"/><path d="M16 9a5 5 0 0 1 0 6"/><path d="M19.364 18.364a9 9 0 0 0 0-12.728"/>"#);
lucide!(MIC, "mic", r#"<path d="M12 19v3"/><path d="M19 10v2a7 7 0 0 1-14 0v-2"/><rect x="9" y="2" width="6" height="13" rx="3"/>"#);
lucide!(FLAG, "flag", r#"<path d="M4 22V4a1 1 0 0 1 .4-.8A6 6 0 0 1 8 2c3 0 5 2 7.333 2q2 0 3.067-.8A1 1 0 0 1 20 4v10a1 1 0 0 1-.4.8A6 6 0 0 1 16 16c-3 0-5-2-8-2a6 6 0 0 0-4 1.528"/>"#);
lucide!(ALERT, "circle-alert", r#"<circle cx="12" cy="12" r="10"/><line x1="12" x2="12" y1="8" y2="12"/><line x1="12" x2="12.01" y1="16" y2="16"/>"#);
lucide!(CHECK, "check", r#"<path d="M20 6 9 17l-5-5"/>"#);
lucide!(SPARK, "sparkles", r#"<path d="M11.017 2.814a1 1 0 0 1 1.966 0l1.051 5.558a2 2 0 0 0 1.594 1.594l5.558 1.051a1 1 0 0 1 0 1.966l-5.558 1.051a2 2 0 0 0-1.594 1.594l-1.051 5.558a1 1 0 0 1-1.966 0l-1.051-5.558a2 2 0 0 0-1.594-1.594l-5.558-1.051a1 1 0 0 1 0-1.966l5.558-1.051a2 2 0 0 0 1.594-1.594z"/><path d="M20 2v4"/><path d="M22 4h-4"/><circle cx="4" cy="20" r="2"/>"#);
lucide!(SEARCH, "search", r#"<path d="m21 21-4.34-4.34"/><circle cx="11" cy="11" r="8"/>"#);
lucide!(FOLDER, "folder", r#"<path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>"#);
lucide!(WAVE, "audio-lines", r#"<path d="M2 10v3"/><path d="M6 6v11"/><path d="M10 3v18"/><path d="M14 8v7"/><path d="M18 5v13"/><path d="M22 10v3"/>"#);

pub fn red() -> Color {
    oklch(0.63, 0.21, 25.0)
}

pub fn tnum(s: TextStyle) -> TextStyle {
    TextStyle { features: "tnum", ..s }
}

/// The height of one line of `s`: what a mark beside that line is centred in.
pub fn line_height(s: TextStyle) -> f32 {
    if s.line_height > 0.0 { s.line_height } else { s.size * 1.25 }
}

pub fn title_style(size: f32) -> TextStyle {
    TextStyle::new(size, 650.0).tracking(-0.2)
}

/// 0:44:12
pub fn ts(t: f64) -> String {
    let t = t.max(0.0) as u64;
    format!("{}:{:02}:{:02}", t / 3600, (t / 60) % 60, t % 60)
}

/// 2 h 34, 50 min, 42 s
pub fn dur(t: f64) -> String {
    let t = t.max(0.0) as u64;
    match (t / 3600, (t / 60) % 60) {
        (0, 0) => format!("{} s", t),
        (0, m) => format!("{m} min"),
        (h, m) => format!("{h} h {m:02}"),
    }
}

/// 3,1 Go, 33 Mo, 864 Ko (in decimal units, like the Finder).
pub fn bytes(n: u64) -> String {
    let n = n as f64;
    let (value, unit) = match n {
        n if n >= 1e9 => (n / 1e9, "Go"),
        n if n >= 1e6 => (n / 1e6, "Mo"),
        n if n >= 1e3 => (n / 1e3, "Ko"),
        n => (n, "octets"),
    };
    let text = if value >= 10.0 || unit == "octets" { format!("{value:.0}") } else { format!("{value:.1}") };
    format!("{} {unit}", text.replace('.', ","))
}

/// A stage's name in the UI.
pub fn step(e: Stage) -> &'static str {
    match e {
        Stage::Transcription => "Transcription",
        Stage::Review => "Relecture IA",
        Stage::Summary => "Bilan IA",
    }
}

/// A meeting's remaining time in the queue (its own and that of the meetings ahead), at least one
/// minute.
pub fn eta(s: &App, id: greffier_core::MeetingId) -> String {
    dur(s.core.time_remaining(id).unwrap_or(0.0).max(60.0))
}

/// A meeting's status as a short line, with where it stands in the queue: « 2ᵉ dans la
/// file · 15 min », « Transcription 40 % · 7 min », otherwise [`status`] in lowercase.
pub fn status_line(s: &App, m: &greffier_core::Meeting) -> String {
    match &m.status {
        Status::Pending(_) => format!("{}ᵉ dans la file · {}", s.core.rank(m.id).map_or(1, |r| r + 1), eta(s, m.id)),
        Status::Running { stage, progress } => format!("{} {:.0} % · {}", step(*stage), progress * 100.0, eta(s, m.id)),
        st => status(st).to_lowercase(),
    }
}

/// A meeting's status in the UI.
pub fn status(s: &Status) -> String {
    match s {
        Status::Recording => "Enregistrement en cours".into(),
        Status::RecordingInterrupted => "Enregistrement interrompu".into(),
        Status::ToProcess => "À traiter".into(),
        Status::Pending(_) => "En attente".into(),
        Status::Running { stage, .. } => step(*stage).into(),
        Status::Failed { stage, .. } => format!("Échec : {}", step(*stage)),
        Status::Done => "Terminée".into(),
    }
}

pub fn spk_color(hue: f32) -> Color {
    oklch(0.7, 0.13, hue)
}

pub fn dot(c: Color, size: f32) -> El {
    el().size(size, size).radius(size / 2.0).fill(c)
}

pub fn glyph(i: &'static Icon, size: f32, c: Color) -> El {
    icon(i, size).color(c)
}

pub fn card(k: &Kit) -> El {
    col().w_full().padding(16.0).gap(10.0).radius(k.radius.lg).fill(k.c.layer_01).border(1.0, k.c.border_muted)
}

pub fn heading(k: &Kit, label: &str) -> El {
    text(label.to_owned()).font(k.ty.ui_medium).color(k.c.text_base)
}

pub fn eyebrow(k: &Kit, label: &str) -> El {
    text(label.to_uppercase()).font(TextStyle::new(10.5, 600.0).tracking(0.6)).color(k.c.text_faint)
}

pub fn muted(k: &Kit, s: impl Into<String>) -> El {
    text(s.into()).font(k.ty.small).color(k.c.text_muted).wrap()
}

/// A clickable « ▶ 0:44:12 » chip that jumps to that moment and starts playback.
pub fn ts_chip(k: &Kit, t: f64, key: String) -> El {
    row()
        .key(key)
        .h(20.0)
        .px(6.0)
        .gap(4.0)
        .center_y()
        .radius(5.0)
        .fill(k.c.layer_02)
        .hover(k.c.hover)
        .press(k.c.pressed)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(format!("Écouter à {}", ts(t)))
        .child(glyph(&PLAY, 9.0, k.c.text_accent))
        .child(text(ts(t)).font(tnum(k.ty.small_medium)).color(k.c.text_accent))
        .on_press(move |s: &mut App| s.seek_and_play(t))
}

pub fn check_row(k: &Kit, ok: bool, label: &str, detail: &str) -> El {
    let c = if ok { k.c.success.icon } else { k.c.warning.icon };
    row()
        .w_full()
        .gap(8.0)
        .center_y()
        .child(glyph(if ok { &CHECK } else { &ALERT }, 13.0, c))
        .child(text(label.to_owned()).font(k.ty.small_medium).color(k.c.text_base))
        .child(text(detail.to_owned()).font(k.ty.small).color(k.c.text_muted).truncate().grow(1.0).min_w(0.0))
}
