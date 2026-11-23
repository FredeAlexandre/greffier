//! `greffier show <réunion>`: a meeting, by default its header and its summary's outline (brief,
//! topics, decisions, number of actions); or one view: the summary, the sheet, the
//! transcription, the speakers, the actions or the passages to check. Utterances, actions and
//! passages are numbered from 1 (`#n`); speakers by their index, as in their label (SPEAKER_00
//! is 0).

use greffier_core::summary::{self, checkboxes, without_front_matter};
use greffier_core::{Decision, Meeting, PassageStatus, Role, Topic, playback_timestamp};
use serde::Serialize;

use crate::library::{Entry, Library, MeetingJson};
use crate::{Context, Failure, HELP, args, text};

const VIEWS: [&str; 6] = ["--bilan", "--transcription", "--fiche", "--locuteurs", "--actions", "--passages"];

pub(crate) fn run(cx: &Context, args: &[String]) -> Result<String, Failure> {
    let a = args::parse(args, &["--json", "--numeros", "--bilan", "--transcription", "--fiche", "--locuteurs", "--actions", "--passages"], &["--from", "--to"])?;
    if a.help() {
        return Ok(HELP.into());
    }
    let views: Vec<&str> = VIEWS.into_iter().filter(|v| a.flag(v)).collect();
    if views.len() > 1 {
        return Err(Failure::Usage(format!("une seule vue à la fois : {}", views.join(" ou "))));
    }
    let view = views.first().copied();
    let windowed = matches!(view, Some("--transcription" | "--passages"));
    if !windowed && (a.value("--from").is_some() || a.value("--to").is_some()) {
        return Err(Failure::Usage("--from et --to vont avec --transcription ou --passages".into()));
    }
    if view != Some("--transcription") && a.flag("--numeros") {
        return Err(Failure::Usage("--numeros va avec --transcription".into()));
    }
    let instant = |option: &str| -> Result<Option<f64>, Failure> {
        a.value(option).map(|t| text::instant(t).ok_or_else(|| Failure::Usage(format!("instant invalide : « {t} » (h:mm:ss, m:ss ou secondes)")))).transpose()
    };
    let window = Window { from: instant("--from")?, to: instant("--to")? };
    let Some(query) = a.positionals.first() else {
        return Err(Failure::Usage("il manque la réunion".into()));
    };
    a.at_most(1)?;
    let library = Library::open(cx)?;
    let e = library.find(query)?;
    let json = a.flag("--json");
    match view {
        None => default(&library, &e, json),
        Some("--bilan") => summary(&e, json),
        Some("--fiche") => sheet(&library, &e, json),
        Some("--transcription") => transcription(&e, window, a.flag("--numeros"), json),
        Some("--locuteurs") => speakers(&e, json),
        Some("--actions") => actions(&e, json),
        _ => passages(&e, window, json),
    }
}

/// The part of the meeting `--from` and `--to` keep: what overlaps it.
#[derive(Clone, Copy)]
struct Window {
    from: Option<f64>,
    to: Option<f64>,
}

impl Window {
    fn keeps(self, start: f64, end: f64) -> bool {
        self.from.is_none_or(|f| end > f) && self.to.is_none_or(|t| start < t)
    }
}

/// The summary without its front matter, or `None` if there is none yet.
fn summary_text(m: &Meeting) -> Option<String> {
    if !m.files.contains(&Role::Summary) {
        return None;
    }
    let mut s = without_front_matter(&m.summary).trim_start_matches(['\n', '\r']).to_string();
    if !s.ends_with('\n') {
        s.push('\n');
    }
    Some(s)
}

fn no_summary(e: &Entry) -> Failure {
    Failure::Failed(format!("pas encore de bilan pour {} {}", e.id, text::completed_stage(&e.meeting)))
}

fn no_transcription(e: &Entry) -> Failure {
    Failure::Failed(format!("pas encore de transcription pour {}", e.id))
}

#[derive(Serialize)]
struct ShowJson {
    #[serde(flatten)]
    meeting: MeetingJson,
    locuteurs: Vec<SpeakerJson>,
    /// The summary's outline; each `null` until there is one.
    en_bref: Option<String>,
    sujets: Option<Vec<MomentJson>>,
    decisions: Option<Vec<MomentJson>>,
    /// The number of actions.
    actions: Option<usize>,
}

/// A topic (`titre`) or a decision (`texte`) of the summary, at its moment.
#[derive(Serialize)]
struct MomentJson {
    /// In seconds.
    debut: f64,
    horodatage: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    titre: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    texte: Option<String>,
}

/// What the default view shows of the summary: the brief, the topics, the decisions and how
/// many actions; the rest is `--bilan`'s, so that a long meeting stays a few lines.
struct Outline {
    brief: String,
    topics: Vec<Topic>,
    decisions: Vec<Decision>,
    actions: usize,
}

impl Outline {
    fn of(m: &Meeting) -> Option<Outline> {
        let md = summary_text(m)?;
        Some(Outline { brief: summary::brief(&md), topics: summary::topics(&md), decisions: summary::decisions(&md), actions: summary::actions(&md).len() })
    }

    fn text(&self) -> String {
        let mut s = String::new();
        if !self.brief.is_empty() {
            s += &format!("En bref : {}\n", self.brief);
        }
        if !self.topics.is_empty() {
            s += "Sujets :\n";
            s += &self.topics.iter().map(|t| format!("  [{}] {}\n", playback_timestamp(t.start), t.title)).collect::<String>();
        }
        match self.decisions.as_slice() {
            [] => s += "Décisions : aucune\n",
            d => {
                s += "Décisions :\n";
                s += &d.iter().map(|d| format!("  [{}] {}\n", playback_timestamp(d.instant), d.text)).collect::<String>();
            }
        }
        s += &format!("Actions : {}\n", self.actions);
        s + "Suite : --bilan (bilan complet), --actions, --transcription --from h:mm:ss --to h:mm:ss (un passage)\n"
    }
}

/// The header (identifier · title; date and time, duration, status; failure; speakers; folder),
/// then the summary's outline or where the meeting stands.
fn default(library: &Library, e: &Entry, json: bool) -> Result<String, Failure> {
    let m = &e.meeting;
    let outline = Outline::of(m);
    if json {
        let moment = |t: f64, titre: Option<String>, texte: Option<String>| MomentJson { debut: t, horodatage: playback_timestamp(t), titre, texte };
        return Ok(crate::json(&ShowJson {
            meeting: MeetingJson::new(e, &library.folder),
            locuteurs: speaker_list(m),
            en_bref: outline.as_ref().map(|o| o.brief.clone()),
            sujets: outline.as_ref().map(|o| o.topics.iter().map(|t| moment(t.start, Some(t.title.clone()), None)).collect()),
            decisions: outline.as_ref().map(|o| o.decisions.iter().map(|d| moment(d.instant, None, Some(d.text.clone()))).collect()),
            actions: outline.as_ref().map(|o| o.actions),
        }));
    }
    let mut s = format!("{} · {}\n{} · {} · {}\n", e.id, m.title, m.start.timestamp(), playback_timestamp(m.duration), text::status(m, e.live.as_ref()));
    if let greffier_core::Status::Failed { message, .. } = &m.status {
        s += &format!("Erreur : {message}\n");
    }
    let shown: Vec<(String, &str)> = m.active_speakers().map(|i| (m.speakers[i].display_name(), m.speakers[i].label.as_str())).collect();
    // A name two voices share comes with their labels.
    let names: Vec<String> = shown.iter().map(|(n, l)| if shown.iter().filter(|(o, _)| o == n).count() > 1 { format!("{n} ({l})") } else { n.clone() }).collect();
    if !names.is_empty() {
        s += &format!("Locuteurs : {}\n", names.join(", "));
    }
    s += &format!("Dossier : {}\n", library.folder.folder(&m.folder_name).display());
    match outline {
        Some(outline) => s += &outline.text(),
        None => {
            s += &format!("Pas encore de bilan {}.\n", text::completed_stage(m));
            if !m.utterances.is_empty() {
                s += "Suite : --transcription [--from h:mm:ss --to h:mm:ss]\n";
            }
        }
    }
    Ok(s)
}

fn summary(e: &Entry, json: bool) -> Result<String, Failure> {
    let s = summary_text(&e.meeting).ok_or_else(|| no_summary(e))?;
    if json {
        return Ok(crate::json(&serde_json::json!({ "id": e.id, "bilan": s })));
    }
    Ok(s)
}

fn sheet(library: &Library, e: &Entry, json: bool) -> Result<String, Failure> {
    let missing = || Failure::Failed(format!("pas de fiche pour {}", e.id));
    if !e.meeting.files.contains(&Role::Sheet) {
        return Err(missing());
    }
    let s = std::fs::read_to_string(library.folder.file(&e.meeting.folder_name, Role::Sheet)).map_err(|_| missing())?;
    if json {
        return Ok(crate::json(&serde_json::json!({ "id": e.id, "fiche": s })));
    }
    Ok(s)
}

#[derive(Serialize)]
struct UtteranceJson {
    numero: usize,
    /// In seconds.
    debut: f64,
    fin: f64,
    /// `0:44:12`
    horodatage: String,
    /// The displayed name (confirmed, otherwise proposed « Khalid ? », otherwise the label).
    locuteur: String,
    /// The displayed speaker's label (merges followed).
    label: String,
    texte: String,
}

fn transcription(e: &Entry, window: Window, numbered: bool, json: bool) -> Result<String, Failure> {
    let m = &e.meeting;
    if m.utterances.is_empty() {
        return Err(no_transcription(e));
    }
    let kept: Vec<UtteranceJson> = m
        .utterances
        .iter()
        .enumerate()
        .filter(|(_, u)| window.keeps(u.start, u.end))
        .map(|(i, u)| {
            let speaker = &m.speakers[m.displayed_speaker(u.speaker)];
            UtteranceJson { numero: i + 1, debut: u.start, fin: u.end, horodatage: playback_timestamp(u.start), locuteur: speaker.display_name(), label: speaker.label.clone(), texte: u.text.clone() }
        })
        .collect();
    if json {
        return Ok(crate::json(&serde_json::json!({ "id": e.id, "repliques": kept })));
    }
    if kept.is_empty() {
        return Ok("Aucune réplique dans cet intervalle.\n".into());
    }
    Ok(kept
        .iter()
        .map(|u| {
            let number = if numbered { format!("#{} ", u.numero) } else { String::new() };
            format!("{number}[{}] {} : {}\n", u.horodatage, u.locuteur, u.texte)
        })
        .collect())
}

#[derive(Serialize)]
struct SpeakerJson {
    /// Its index, as in its label.
    index: usize,
    label: String,
    /// The name confirmed by the user (merges followed).
    nom: Option<String>,
    /// The name the summary proposes, not confirmed.
    proposition: Option<String>,
    /// What the UI displays: « Alexandre », « Khalid ? », « SPEAKER_02 ».
    affiche: String,
    /// The label of the speaker it is merged into.
    fusionne_dans: Option<String>,
    /// Seconds of speech (0 for a merged speaker: counted with the one replacing it).
    temps_de_parole: u64,
}

/// Seconds of speech of a displayed speaker, merged ones included.
fn speech(m: &Meeting, i: usize) -> f64 {
    m.utterances.iter().filter(|u| m.displayed_speaker(u.speaker) == i).map(|u| u.end - u.start).sum()
}

fn speaker_list(m: &Meeting) -> Vec<SpeakerJson> {
    (0..m.speakers.len())
        .map(|i| {
            let shown = &m.speakers[m.displayed_speaker(i)];
            let merged = m.speakers[i].merged_into.map(|j| m.speakers[j].label.clone());
            SpeakerJson {
                index: i,
                label: m.speakers[i].label.clone(),
                nom: shown.name.clone(),
                proposition: shown.proposal.clone().filter(|_| shown.name.is_none()),
                affiche: shown.display_name(),
                temps_de_parole: if merged.is_some() { 0 } else { speech(m, i).round() as u64 },
                fusionne_dans: merged,
            }
        })
        .collect()
}

fn speakers(e: &Entry, json: bool) -> Result<String, Failure> {
    let m = &e.meeting;
    if m.speakers.is_empty() {
        return Err(no_transcription(e));
    }
    let list = speaker_list(m);
    if json {
        return Ok(crate::json(&serde_json::json!({ "id": e.id, "locuteurs": list })));
    }
    let total: f64 = m.utterances.iter().map(|u| u.end - u.start).sum();
    Ok(list
        .iter()
        .map(|l| match &l.fusionne_dans {
            Some(into) => format!("{} · {} · {} · fusionné dans {into}\n", l.index, l.label, l.affiche),
            None => {
                let own = speech(m, l.index);
                let share = if total > 0.0 { own / total * 100.0 } else { 0.0 };
                format!("{} · {} · {} · {} · {share:.0} %\n", l.index, l.label, l.affiche, playback_timestamp(own))
            }
        })
        .collect())
}

fn actions(e: &Entry, json: bool) -> Result<String, Failure> {
    if !e.meeting.files.contains(&Role::Summary) {
        return Err(no_summary(e));
    }
    let boxes = checkboxes(&e.meeting.summary);
    if json {
        let list: Vec<_> = boxes.iter().enumerate().map(|(i, (done, t))| serde_json::json!({ "numero": i + 1, "fait": done, "texte": t })).collect();
        return Ok(crate::json(&serde_json::json!({ "id": e.id, "actions": list })));
    }
    if boxes.is_empty() {
        return Ok("Aucune action.\n".into());
    }
    Ok(boxes.iter().enumerate().map(|(i, (done, t))| format!("#{} [{}] {t}\n", i + 1, if *done { "x" } else { " " })).collect())
}

#[derive(Serialize)]
struct PassageJson {
    numero: usize,
    /// The number of its utterance (as in `--transcription --numeros`).
    replique: usize,
    /// Its utterance's start, in seconds.
    debut: f64,
    horodatage: String,
    extrait: String,
    hypothese: Option<String>,
    /// What the relisten heard.
    reecoute: String,
    /// `a_verifier`, `hypothese_acceptee`, `original_garde` or `corrige`.
    statut: &'static str,
    /// The user's correction, if `corrige`.
    correction: Option<String>,
}

fn passages(e: &Entry, window: Window, json: bool) -> Result<String, Failure> {
    let m = &e.meeting;
    if m.utterances.is_empty() {
        return Err(no_transcription(e));
    }
    let kept: Vec<PassageJson> = m
        .passages
        .iter()
        .enumerate()
        .filter_map(|(i, p)| {
            let u = m.utterances.get(p.utterance)?;
            window.keeps(u.start, u.end).then(|| {
                let (statut, correction) = match &p.status {
                    PassageStatus::Pending => ("a_verifier", None),
                    PassageStatus::HypothesisAccepted => ("hypothese_acceptee", None),
                    PassageStatus::OriginalKept => ("original_garde", None),
                    PassageStatus::CorrectedManually(t) => ("corrige", Some(t.clone())),
                };
                PassageJson {
                    numero: i + 1,
                    replique: p.utterance + 1,
                    debut: u.start,
                    horodatage: playback_timestamp(u.start),
                    extrait: p.excerpt.clone(),
                    hypothese: p.hypothesis.clone(),
                    reecoute: p.relisten.clone(),
                    statut,
                    correction,
                }
            })
        })
        .collect();
    if json {
        return Ok(crate::json(&serde_json::json!({ "id": e.id, "passages": kept })));
    }
    if kept.is_empty() {
        return Ok(if m.passages.is_empty() { "Aucun passage à vérifier.\n" } else { "Aucun passage à vérifier dans cet intervalle.\n" }.into());
    }
    Ok(kept
        .iter()
        .map(|p| {
            let what = match (p.statut, &p.hypothese, &p.correction) {
                ("a_verifier", Some(h), _) => format!("à vérifier : « {} » (hypothèse : « {h} »)", p.extrait),
                ("a_verifier", None, _) => format!("à vérifier : « {} »", p.extrait),
                ("hypothese_acceptee", h, _) => format!("hypothèse acceptée : « {} » → « {} »", p.extrait, h.as_deref().unwrap_or(&p.extrait)),
                ("corrige", _, Some(c)) => format!("corrigé : « {} » → « {c} »", p.extrait),
                _ => format!("gardé tel quel : « {} »", p.extrait),
            };
            format!("#{} [{}] {what} · réplique #{}\n", p.numero, p.horodatage, p.replique)
        })
        .collect())
}
