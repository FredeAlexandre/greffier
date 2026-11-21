//! Le bilan est du Markdown écrit par le LLM à partir d'un template (celui de `prompt-bilan.md`).
//! L'app ne stocke que ce texte. Elle en tire ce dont l'interface a besoin : les sujets
//! (chapitres de la timeline), les décisions (◆), le résumé, et rend les horodatages
//! « (0:44:12) » cliquables.

/// Lit « h:mm:ss » au début de `s`. Renvoie les secondes et la longueur lue.
fn parse_ts(s: &str) -> Option<(f64, usize)> {
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
fn stamps(line: &str) -> Vec<(usize, usize, f64)> {
    let mut out = Vec::new();
    for (i, _) in line.match_indices('(') {
        if let Some((t, n)) = parse_ts(&line[i + 1..]) {
            if line[i + 1 + n..].starts_with(')') {
                out.push((i, n + 2, t));
            }
        }
    }
    out
}

fn strip(s: &str) -> String {
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

/// Les sujets : les `### Titre (h:mm:ss)` de « Sujets abordés ».
pub fn topics(md: &str) -> Vec<(f64, String)> {
    let mut v: Vec<(f64, String)> = section(md, "Sujets")
        .filter_map(|l| {
            let h = l.strip_prefix("### ")?;
            let (i, _, t) = *stamps(h).first()?;
            Some((t, strip(&h[..i])))
        })
        .collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    v
}

/// Les décisions : les puces horodatées de « Décisions ».
pub fn decisions(md: &str) -> Vec<(f64, String)> {
    section(md, "Décisions")
        .filter_map(|l| {
            let item = l.trim_start().strip_prefix("- ")?;
            let (i, n, t) = *stamps(item).first()?;
            let text = format!("{}{}", &item[..i], &item[i + n..]);
            Some((t, strip(&text)))
        })
        .collect()
}

/// Le premier paragraphe de « En bref ».
pub fn brief(md: &str) -> String {
    section(md, "En bref").map(str::trim).skip_while(|l| l.is_empty()).take_while(|l| !l.is_empty()).collect::<Vec<_>>().join(" ")
}

/// Rend les horodatages cliquables : « (0:44:12) » devient un lien `t:2652`.
pub fn enrich(md: &str) -> String {
    let mut out = String::with_capacity(md.len() + 256);
    for line in md.lines() {
        let mut last = 0;
        for (i, n, t) in stamps(line) {
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

/// Coche ou décoche la `index`-ième case `- [ ]` du Markdown.
pub fn set_task(md: &mut String, index: usize, done: bool) {
    let mut n = 0;
    let mut out = String::with_capacity(md.len());
    for line in md.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("- [ ]") || trimmed.starts_with("- [x]") || trimmed.starts_with("- [X]") {
            if n == index {
                let pad = &line[..line.len() - trimmed.len()];
                out.push_str(pad);
                out.push_str(if done { "- [x]" } else { "- [ ]" });
                out.push_str(&trimmed[5..]);
                out.push('\n');
                n += 1;
                continue;
            }
            n += 1;
        }
        out.push_str(line);
        out.push('\n');
    }
    *md = out;
}

/// L'index de la case dont le `[` est à `offset` dans `src` (pour le rappel `on_task`).
pub fn task_index(src: &str, offset: usize) -> usize {
    src[..offset.min(src.len())]
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with("- [ ]") || t.starts_with("- [x]") || t.starts_with("- [X]")
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MD: &str = "## En bref\nUn.\nDeux.\n\n## Sujets abordés\n### Bandeau (0:00:01)\n- a\n### Search **fermé** (1:05:17)\n## Décisions\n- (0:44:12) On vend.\n- Rien\n## Actions\n- [ ] A\n- [x] B\n";

    #[test]
    fn reads_the_template() {
        assert_eq!(topics(MD), vec![(1.0, "Bandeau".into()), (3917.0, "Search fermé".into())]);
        assert_eq!(decisions(MD), vec![(2652.0, "On vend.".into())]);
        assert_eq!(brief(MD), "Un. Deux.");
        assert!(enrich(MD).contains("[▶ 0:44:12](t:2652)"));
        let mut m = MD.to_string();
        set_task(&mut m, 1, false);
        set_task(&mut m, 0, true);
        assert!(m.contains("- [x] A\n- [ ] B"));
    }
}
