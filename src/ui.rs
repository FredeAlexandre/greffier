//! Petits morceaux d'interface partagés : icônes, formats de temps, cartes, puces d'horodatage.

use rune::tok::TextStyle;
use rune::{Color, El, Icon, col, el, icon, oklch, row, text};
use greffier_coeur::{Etape, Statut};
use rune_kit::Kit;

use crate::App;

macro_rules! icon_def {
    ($name:ident, $id:literal, $body:literal) => {
        pub static $name: Icon = Icon { name: $id, view: 16.0, body: $body };
    };
}

icon_def!(PLAY, "g-play", r#"<path d="M5 3.4V12.6L12.6 8Z" fill="currentColor" stroke="currentColor" stroke-width="1" stroke-linejoin="round"/>"#);
icon_def!(PAUSE, "g-pause", r#"<rect x="4" y="3.5" width="2.6" height="9" rx="0.8" fill="currentColor"/><rect x="9.4" y="3.5" width="2.6" height="9" rx="0.8" fill="currentColor"/>"#);
icon_def!(BACK, "g-back", r#"<path d="M3.8 8.2A4.3 4.3 0 1 0 5.1 5.1" stroke="currentColor" stroke-width="1.3" fill="none" stroke-linecap="round"/><path d="M4.9 2.6V5.4H7.7" stroke="currentColor" stroke-width="1.3" fill="none" stroke-linecap="round" stroke-linejoin="round"/>"#);
icon_def!(FORWARD, "g-forward", r#"<path d="M12.2 8.2A4.3 4.3 0 1 1 10.9 5.1" stroke="currentColor" stroke-width="1.3" fill="none" stroke-linecap="round"/><path d="M11.1 2.6V5.4H8.3" stroke="currentColor" stroke-width="1.3" fill="none" stroke-linecap="round" stroke-linejoin="round"/>"#);
icon_def!(EXPAND, "g-expand", r#"<path d="M3 6.2V3H6.2M9.8 3H13V6.2M13 9.8V13H9.8M6.2 13H3V9.8" stroke="currentColor" stroke-width="1.3" fill="none" stroke-linecap="round" stroke-linejoin="round"/>"#);
icon_def!(SHRINK, "g-shrink", r#"<path d="M6.2 3V6.2H3M13 6.2H9.8V3M9.8 13V9.8H13M3 9.8H6.2V13" stroke="currentColor" stroke-width="1.3" fill="none" stroke-linecap="round" stroke-linejoin="round"/>"#);
icon_def!(RECORD, "g-record", r#"<circle cx="8" cy="8" r="4.6" fill="currentColor"/>"#);
icon_def!(STOP, "g-stop", r#"<rect x="4" y="4" width="8" height="8" rx="1.6" fill="currentColor"/>"#);
icon_def!(HOME, "g-home", r#"<path d="M3 7L8 3L13 7V12.5H9.5V9.5H6.5V12.5H3Z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" fill="none"/>"#);
icon_def!(SLIDERS, "g-sliders", r#"<path d="M3 5H13M3 11H13" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/><circle cx="6" cy="5" r="1.6" fill="currentColor"/><circle cx="10.5" cy="11" r="1.6" fill="currentColor"/>"#);
icon_def!(SOUND, "g-sound", r#"<path d="M2.8 6.2H5L7.8 3.9V12.1L5 9.8H2.8Z" fill="currentColor"/><path d="M10.2 5.7A3.2 3.2 0 0 1 10.2 10.3M11.9 4.1A5.4 5.4 0 0 1 11.9 11.9" stroke="currentColor" stroke-width="1.2" fill="none" stroke-linecap="round"/>"#);
icon_def!(MIC, "g-mic", r#"<rect x="6" y="2.4" width="4" height="7" rx="2" stroke="currentColor" stroke-width="1.2" fill="none"/><path d="M3.9 7.6A4.1 4.1 0 0 0 12.1 7.6M8 11.7V13.6" stroke="currentColor" stroke-width="1.2" fill="none" stroke-linecap="round"/>"#);
icon_def!(FLAG, "g-flag", r#"<path d="M4 13.6V2.6M4 3H11.6L10 5.7L11.6 8.4H4" stroke="currentColor" stroke-width="1.2" fill="none" stroke-linecap="round" stroke-linejoin="round"/>"#);
icon_def!(CHECK, "g-check", r#"<path d="M3.6 8.4L6.6 11.2L12.4 4.8" stroke="currentColor" stroke-width="1.5" fill="none" stroke-linecap="round" stroke-linejoin="round"/>"#);
icon_def!(SPARK, "g-spark", r#"<path d="M8 2.5L9.3 6.7L13.5 8L9.3 9.3L8 13.5L6.7 9.3L2.5 8L6.7 6.7Z" fill="currentColor"/>"#);
icon_def!(SEARCH, "g-search", r#"<circle cx="7" cy="7" r="3.75" stroke="currentColor" stroke-width="1.2" fill="none"/><path d="M10 10L12.75 12.75" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>"#);
icon_def!(FOLDER, "g-folder", r#"<path d="M2.5 4.6C2.5 4 3 3.5 3.6 3.5H6.3L7.6 5H12.4C13 5 13.5 5.5 13.5 6.1V11.4C13.5 12 13 12.5 12.4 12.5H3.6C3 12.5 2.5 12 2.5 11.4Z" stroke="currentColor" stroke-width="1.2" fill="none" stroke-linejoin="round"/>"#);
icon_def!(WAVE, "g-wave", r#"<path d="M2.5 8H3.5M5 5.5V10.5M7 3.5V12.5M9 6V10M11 4.5V11.5M13 7V9" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>"#);

pub fn red() -> Color {
    oklch(0.63, 0.21, 25.0)
}

pub fn tnum(s: TextStyle) -> TextStyle {
    TextStyle { features: "tnum", ..s }
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

/// 3,1 Go, 33 Mo, 864 Ko (en unités décimales, comme le Finder).
pub fn octets(n: u64) -> String {
    let n = n as f64;
    let (valeur, unite) = match n {
        n if n >= 1e9 => (n / 1e9, "Go"),
        n if n >= 1e6 => (n / 1e6, "Mo"),
        n if n >= 1e3 => (n / 1e3, "Ko"),
        n => (n, "octets"),
    };
    let texte = if valeur >= 10.0 || unite == "octets" { format!("{valeur:.0}") } else { format!("{valeur:.1}") };
    format!("{} {unite}", texte.replace('.', ","))
}

/// Le nom d'une étape dans l'interface.
pub fn step(e: Etape) -> &'static str {
    match e {
        Etape::Transcription => "Transcription",
        Etape::Relecture => "Relecture IA",
        Etape::Bilan => "Bilan IA",
    }
}

/// Le statut d'une réunion dans l'interface.
pub fn status(s: &Statut) -> String {
    match s {
        Statut::EnregistrementEnCours => "Enregistrement en cours".into(),
        Statut::EnregistrementInterrompu => "Enregistrement interrompu".into(),
        Statut::ATraiter => "À traiter".into(),
        Statut::EnAttente(_) => "En attente".into(),
        Statut::EnCours { etape, .. } => step(*etape).into(),
        Statut::Echec { etape, .. } => format!("Échec : {}", step(*etape)),
        Statut::Terminee => "Terminée".into(),
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

/// Une puce cliquable « ▶ 0:44:12 » qui saute à ce moment et lance la lecture.
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
        .child(glyph(if ok { &CHECK } else { &FLAG }, 13.0, c))
        .child(text(label.to_owned()).font(k.ty.small_medium).color(k.c.text_base))
        .child(text(detail.to_owned()).font(k.ty.small).color(k.c.text_muted).truncate().grow(1.0).min_w(0.0))
}
