//! Le bilan est du Markdown écrit par l'agent à partir du prompt du bilan. Greffier ne stocke
//! que ce texte. Il en tire ce dont il a besoin : les sujets (chapitres de la timeline), les
//! décisions (◆), le résumé, les actions, et rend les horodatages « (0:44:12) » cliquables.

use crate::reunion::{Decision, Sujet};

/// Lit « h:mm:ss » au début de `s`. Renvoie les secondes et la longueur lue.
fn lire_horodatage(s: &str) -> Option<(f64, usize)> {
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
fn horodatages(line: &str) -> Vec<(usize, usize, f64)> {
    let mut out = Vec::new();
    for (i, _) in line.match_indices('(') {
        if let Some((t, n)) = lire_horodatage(&line[i + 1..])
            && line[i + 1 + n..].starts_with(')')
        {
            out.push((i, n + 2, t));
        }
    }
    out
}

fn nettoyer(s: &str) -> String {
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

fn est_une_case(ligne: &str) -> bool {
    let t = ligne.trim_start();
    t.starts_with("- [ ]") || t.starts_with("- [x]") || t.starts_with("- [X]")
}

/// Les sujets : les `### Titre (h:mm:ss)` de « Sujets abordés ».
pub fn sujets(md: &str) -> Vec<Sujet> {
    let mut v: Vec<Sujet> = section(md, "Sujets")
        .filter_map(|l| {
            let h = l.strip_prefix("### ")?;
            let (i, _, t) = *horodatages(h).first()?;
            Some(Sujet { debut: t, titre: nettoyer(&h[..i]) })
        })
        .collect();
    v.sort_by(|a, b| a.debut.total_cmp(&b.debut));
    v
}

/// Les décisions : les puces horodatées de « Décisions ».
pub fn decisions(md: &str) -> Vec<Decision> {
    section(md, "Décisions")
        .filter_map(|l| {
            let item = l.trim_start().strip_prefix("- ")?;
            let (i, n, t) = *horodatages(item).first()?;
            let text = format!("{}{}", &item[..i], &item[i + n..]);
            Some(Decision { instant: t, texte: nettoyer(&text) })
        })
        .collect()
}

/// Les actions : les cases `- [ ]` de la section « Actions ».
pub fn actions(md: &str) -> Vec<String> {
    section(md, "Actions").filter(|l| est_une_case(l)).map(|l| nettoyer(&l.trim_start()[5..])).collect()
}

/// Le premier paragraphe de « En bref ».
pub fn en_bref(md: &str) -> String {
    section(md, "En bref").map(str::trim).skip_while(|l| l.is_empty()).take_while(|l| !l.is_empty()).collect::<Vec<_>>().join(" ")
}

/// Ce qu'il manque au bilan pour que l'interface s'en serve, ou rien.
pub fn manques(md: &str) -> Option<String> {
    let mut manque = Vec::new();
    if sujets(md).is_empty() {
        manque.push("sujets horodatés");
    }
    if decisions(md).is_empty() {
        manque.push("décisions horodatées");
    }
    if actions(md).is_empty() {
        manque.push("actions");
    }
    (!manque.is_empty()).then(|| format!("Le bilan n'a pas de {}.", manque.join(", ni de ")))
}

/// Rend les horodatages cliquables : « (0:44:12) » devient un lien `t:2652`.
pub fn avec_liens_de_lecture(md: &str) -> String {
    let mut out = String::with_capacity(md.len() + 256);
    for line in md.lines() {
        let mut last = 0;
        for (i, n, t) in horodatages(line) {
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
pub fn cocher(md: &mut String, index: usize, faite: bool) {
    let mut n = 0;
    let mut out = String::with_capacity(md.len());
    for line in md.lines() {
        if est_une_case(line) {
            if n == index {
                let trimmed = line.trim_start();
                let pad = &line[..line.len() - trimmed.len()];
                out.push_str(pad);
                out.push_str(if faite { "- [x]" } else { "- [ ]" });
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

/// L'index de la case dont le `[` est à `offset` dans `src`.
pub fn index_de_la_case(src: &str, offset: usize) -> usize {
    src[..offset.min(src.len())].lines().filter(|l| est_une_case(l)).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MD: &str = "## En bref\nUn.\nDeux.\n\n## Sujets abordés\n### Bandeau (0:00:01)\n- a\n### Search **fermé** (1:05:17)\n## Décisions\n- (0:44:12) On vend.\n- Rien\n## Actions\n- [ ] A\n- [x] B\n";

    #[test]
    fn lit_le_modele_du_bilan() {
        assert_eq!(sujets(MD), vec![Sujet { debut: 1.0, titre: "Bandeau".into() }, Sujet { debut: 3917.0, titre: "Search fermé".into() }]);
        assert_eq!(decisions(MD), vec![Decision { instant: 2652.0, texte: "On vend.".into() }]);
        assert_eq!(en_bref(MD), "Un. Deux.");
        assert_eq!(actions(MD), vec!["A".to_string(), "B".to_string()]);
        assert_eq!(manques(MD), None);
        assert!(avec_liens_de_lecture(MD).contains("[▶ 0:44:12](t:2652)"));
        let mut m = MD.to_string();
        cocher(&mut m, 1, false);
        cocher(&mut m, 0, true);
        assert!(m.contains("- [x] A\n- [ ] B"));
    }
}
