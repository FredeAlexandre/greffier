//! Le bilan est du Markdown écrit par l'agent à partir du prompt du bilan. Greffier ne stocke
//! que ce texte. Il en tire ce dont il a besoin : les sujets (chapitres de la timeline), les
//! décisions (◆), le résumé, les actions, et rend les horodatages « (0:44:12) » cliquables.

use crate::meeting::{Decision, Topic};

/// Lit « h:mm:ss » au début de `s`. Renvoie les secondes et la longueur lue.
fn read_timestamp(s: &str) -> Option<(f64, usize)> {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 || i > 2 || b.len() < i + 6 || b[i] != b':' || b[i + 3] != b':' {
        return None;
    }
    let digits = |a: usize| b[a].is_ascii_digit() && b[a + 1].is_ascii_digit();
    if !digits(i + 1) || !digits(i + 4) {
        return None;
    }
    let h: f64 = s[..i].parse().ok()?;
    let m: f64 = s[i + 1..i + 3].parse().ok()?;
    let sec: f64 = s[i + 4..i + 6].parse().ok()?;
    Some((h * 3600.0 + m * 60.0 + sec, i + 6))
}

/// Tous les « (h:mm:ss) » d'une ligne : (position de la parenthèse ouvrante, longueur, secondes).
fn timestamps(line: &str) -> Vec<(usize, usize, f64)> {
    let mut out = Vec::new();
    for (i, _) in line.match_indices('(') {
        if let Some((t, n)) = read_timestamp(&line[i + 1..])
            && line[i + 1 + n..].starts_with(')')
        {
            out.push((i, n + 2, t));
        }
    }
    out
}

/// Le premier « (h:mm:ss) » d'une ligne, en secondes.
pub(crate) fn first_timestamp(line: &str) -> Option<f64> {
    timestamps(line).first().map(|h| h.2)
}

fn clean(s: &str) -> String {
    s.replace("**", "").trim().to_string()
}

/// Les lignes d'une section `## Titre` (jusqu'au prochain `## `).
fn section<'a>(md: &'a str, title: &str) -> impl Iterator<Item = &'a str> {
    let mut inside = false;
    md.lines().filter(move |l| {
        if let Some(h) = l.strip_prefix("## ") {
            inside = h.trim().starts_with(title);
            return false;
        }
        inside
    })
}

fn is_checkbox(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("- [ ]") || t.starts_with("- [x]") || t.starts_with("- [X]")
}

/// Les sujets : les `### Titre (h:mm:ss)` de « Sujets abordés ».
pub fn topics(md: &str) -> Vec<Topic> {
    let mut v: Vec<Topic> = section(md, "Sujets")
        .filter_map(|l| {
            let h = l.strip_prefix("### ")?;
            let (i, _, t) = *timestamps(h).first()?;
            Some(Topic { start: t, title: clean(&h[..i]) })
        })
        .collect();
    v.sort_by(|a, b| a.start.total_cmp(&b.start));
    v
}

/// Les décisions : les puces horodatées de « Décisions ».
pub fn decisions(md: &str) -> Vec<Decision> {
    section(md, "Décisions")
        .filter_map(|l| {
            let item = l.trim_start().strip_prefix("- ")?;
            let (i, n, t) = *timestamps(item).first()?;
            let text = format!("{}{}", &item[..i], &item[i + n..]);
            Some(Decision { instant: t, text: clean(&text) })
        })
        .collect()
}

/// Les actions : les cases `- [ ]` de la section « Actions ».
pub fn actions(md: &str) -> Vec<String> {
    section(md, "Actions").filter(|l| is_checkbox(l)).map(|l| clean(&l.trim_start()[5..])).collect()
}

/// Le premier paragraphe de « En bref ».
pub fn brief(md: &str) -> String {
    section(md, "En bref").map(str::trim).skip_while(|l| l.is_empty()).take_while(|l| !l.is_empty()).collect::<Vec<_>>().join(" ")
}

/// Ce qu'il manque au bilan pour que l'interface s'en serve, ou rien.
pub fn missing_parts(md: &str) -> Option<String> {
    let mut missing = Vec::new();
    if topics(md).is_empty() {
        missing.push("sujets horodatés");
    }
    if decisions(md).is_empty() {
        missing.push("décisions horodatées");
    }
    if actions(md).is_empty() {
        missing.push("actions");
    }
    (!missing.is_empty()).then(|| format!("Le bilan n'a pas de {}.", missing.join(", ni de ")))
}

/// Rend les horodatages cliquables : « (0:44:12) » devient un lien `t:2652`.
pub fn with_playback_links(md: &str) -> String {
    let mut out = String::with_capacity(md.len() + 256);
    for line in md.lines() {
        let mut last = 0;
        for (i, n, t) in timestamps(line) {
            out.push_str(&line[last..i]);
            let label = &line[i + 1..i + n - 1];
            out.push_str(&format!("[▶ {label}](t:{})", t as u64));
            last = i + n;
        }
        out.push_str(&line[last..]);
        out.push('\n');
    }
    out
}

/// La position, en octets, du caractère entre les crochets de la `index`-ième case `- [ ]`.
pub fn checkbox_position(md: &str, index: usize) -> Option<usize> {
    let mut start = 0;
    let mut n = 0;
    for line in md.split_inclusive('\n') {
        if is_checkbox(line) {
            if n == index {
                return Some(start + (line.len() - line.trim_start().len()) + 3);
            }
            n += 1;
        }
        start += line.len();
    }
    None
}

/// Le nombre de cases `- [ ]` du Markdown.
pub fn checkbox_count(md: &str) -> usize {
    md.lines().filter(|l| is_checkbox(l)).count()
}

/// Coche ou décoche la `index`-ième case `- [ ]` du Markdown, sans toucher au reste. Vrai si
/// la case a changé.
pub fn tick(md: &mut String, index: usize, done: bool) -> bool {
    let Some(p) = checkbox_position(md, index) else { return false };
    if (md.as_bytes()[p] != b' ') == done {
        return false;
    }
    md.replace_range(p..p + 1, if done { "x" } else { " " });
    true
}

/// L'index de la case dont le `[` est à `offset` dans `src`.
pub fn checkbox_index(src: &str, offset: usize) -> usize {
    src[..offset.min(src.len())].lines().filter(|l| is_checkbox(l)).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MD: &str = "## En bref\nUn.\nDeux.\n\n## Sujets abordés\n### Bandeau (0:00:01)\n- a\n### Search **fermé** (1:05:17)\n## Décisions\n- (0:44:12) On vend.\n- Rien\n## Actions\n- [ ] A\n- [x] B\n";

    #[test]
    fn reads_the_summary_template() {
        assert_eq!(topics(MD), vec![Topic { start: 1.0, title: "Bandeau".into() }, Topic { start: 3917.0, title: "Search fermé".into() }]);
        assert_eq!(decisions(MD), vec![Decision { instant: 2652.0, text: "On vend.".into() }]);
        assert_eq!(brief(MD), "Un. Deux.");
        assert_eq!(actions(MD), vec!["A".to_string(), "B".to_string()]);
        assert_eq!(missing_parts(MD), None);
        assert!(with_playback_links(MD).contains("[▶ 0:44:12](t:2652)"));
        let mut m = MD.to_string();
        tick(&mut m, 1, false);
        tick(&mut m, 0, true);
        assert!(m.contains("- [x] A\n- [ ] B"));
    }
}
