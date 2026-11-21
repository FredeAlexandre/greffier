//! Données fictives du prototype, en attendant le dossier de réunions. La forme suit ce que
//! produisait l'outil « Réunions » du vault : locuteurs SPEAKER_xx avec un nom proposé, répliques
//! horodatées, passages à vérifier, sujets horodatés, décisions, actions et questions ouvertes.
//!
//! [`adaptateurs`] branche les faux adaptateurs du prototype : une capture au temps accéléré, un
//! transcripteur et un séparateur qui inventent une réunion courte, un agent qui en écrit le bilan.

use crate::adaptateurs::{Adaptateurs, DemandeDeBilan, EtatDeRessource, Participant, Ressource, ResultatDeRelecture, ResultatDuBilan};
use crate::faux::{FausseCapture, FausseCorbeille, FausseHorloge, FaussesRessources, FauxAgent, FauxSeparateur, FauxTranscripteur};
use crate::reunion::{Etape, Locuteur, Mot, PassageAVerifier, Repere, Replique, Reunion, Statut, StatutDuPassage, TourDeParole};
use crate::temps::{DateHeure, hms};
use crate::{bilan, dossier};

/// Aujourd'hui, pour les données fictives : samedi 21 novembre 2026, 14h02.
pub const AUJOURD_HUI: DateHeure = DateHeure::new(2026, 11, 21, 14, 2);

/// Le temps de l'enregistrement va 20 fois plus vite que l'horloge, pour la démo.
pub const VITESSE_D_ENREGISTREMENT: f64 = 20.0;

/// Durée simulée d'une étape, en secondes.
pub fn duree_simulee(etape: Etape) -> f64 {
    match etape {
        Etape::Transcription => 6.0,
        Etape::Relecture => 4.5,
        Etape::Bilan => 4.5,
    }
}

/// Les personnes du lexique, proposées quand on nomme un locuteur.
pub const PERSONNES: [&str; 7] = ["Alexandre", "Jérôme", "Khalid", "Pablo", "Aref", "Nico", "Samuel"];

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (self.next() % 10_000) as f64 / 10_000.0 * (hi - lo)
    }
    fn pick(&mut self, weights: &[f64]) -> usize {
        let total: f64 = weights.iter().sum();
        let mut r = self.range(0.0, total);
        for (i, w) in weights.iter().enumerate() {
            if r < *w {
                return i;
            }
            r -= w;
        }
        weights.len() - 1
    }
}

const FILLERS: [&str; 24] = [
    "Ouais, d'accord.",
    "Attends, je partage mon écran, tu vois bien là ?",
    "Ok, ça marche pour moi.",
    "Donc si je résume, on garde ça pour la prochaine release.",
    "Je regarde en même temps sur staging, c'est bien le cas.",
    "Non mais c'est logique, il faut juste qu'on le documente quelque part.",
    "Tu peux revenir sur la slide d'avant ?",
    "Ça, c'est le cas qu'on a eu la semaine dernière avec le client.",
    "Je note, je mets un ticket dans Linear.",
    "C'est pas bloquant, mais il faut le garder en tête.",
    "Là, on voit bien que la colonne est vide pour la moitié des marques.",
    "Moi je trouve que c'est plus clair comme ça, honnêtement.",
    "Est-ce qu'on a une idée du volume que ça représente ?",
    "Pas encore, je peux sortir les chiffres d'ici demain.",
    "Ok. Et côté sales, ça change quoi concrètement ?",
    "Ça veut dire qu'on peut le vendre dès maintenant.",
    "Je suis d'accord, mais pas avant d'avoir testé avec deux ou trois clients.",
    "On l'avait déjà évoqué en début de semaine.",
    "Il faut que ce soit cohérent partout dans l'appli, sinon ça va perdre les gens.",
    "Je vous montre, c'est rapide.",
    "Hmm. Je ne suis pas sûr que ce soit le bon endroit pour ça.",
    "C'est noté. On passe au point suivant ?",
    "Oui, vas-y.",
    "Juste une question avant : ça part en prod quand ?",
];

/// Remplit les trous entre les répliques clés avec des répliques de remplissage, pour que la
/// timeline et la transcription aient la densité d'une vraie réunion.
fn weave(keys: Vec<Replique>, duration: f64, weights: &[f64], seed: u64) -> Vec<Replique> {
    let mut rng = Lcg(seed);
    let mut out = Vec::new();
    let mut t = 1.0;
    let mut keys = keys.into_iter().peekable();
    let mut last = usize::MAX;
    while t < duration - 2.0 {
        if let Some(k) = keys.peek()
            && t >= k.debut - 6.0
        {
            let k = keys.next().unwrap();
            t = k.fin + rng.range(0.5, 2.5);
            last = k.locuteur;
            out.push(k);
            continue;
        }
        let limit = keys.peek().map_or(duration - 1.0, |k| k.debut - 1.0);
        let len = rng.range(5.0, 55.0).min(limit - t).max(2.0);
        let mut sp = rng.pick(weights);
        if sp == last {
            sp = rng.pick(weights);
        }
        let text = FILLERS[(rng.next() % FILLERS.len() as u64) as usize].to_string();
        out.push(Replique { locuteur: sp, debut: t, fin: t + len, texte: text, passage: None });
        last = sp;
        t += len + rng.range(0.3, 3.0);
    }
    out
}

fn u(speaker: usize, start: f64, len: f64, text: &str) -> Replique {
    Replique { locuteur: speaker, debut: start, fin: start + len, texte: text.to_string(), passage: None }
}

fn sp(label: &str, guess: &str, evidence: &str, hue: f32, confirmed: bool) -> Locuteur {
    Locuteur {
        nom: confirmed.then(|| guess.to_string()),
        proposition: Some(guess.into()),
        justification: evidence.into(),
        ..Locuteur::new(label, hue)
    }
}

/// Relie chaque passage douteux à la réplique qui contient son extrait.
fn attach_doubts(utterances: &mut [Replique], doubts: &mut [PassageAVerifier]) {
    for (di, d) in doubts.iter_mut().enumerate() {
        if let Some(ui) = utterances.iter().position(|x| x.texte.contains(&d.extrait) && x.passage.is_none()) {
            utterances[ui].passage = Some(di);
            d.replique = ui;
        }
    }
}

fn doubt(excerpt: &str, hypothesis: Option<&str>, relisten: &str) -> PassageAVerifier {
    PassageAVerifier { replique: 0, extrait: excerpt.into(), hypothese: hypothesis.map(Into::into), reecoute: relisten.into(), statut: StatutDuPassage::EnAttente }
}

/// Une réunion entièrement traitée.
fn terminee(id: u32, debut: DateHeure, duree: f64, titre: &str) -> Reunion {
    Reunion {
        titre: titre.into(),
        statut: Statut::Terminee,
        etape_faite: Some(Etape::Bilan),
        nombre_de_locuteurs: Some(4),
        ..Reunion::new(id, dossier::nom_de_dossier(debut, titre), debut, duree)
    }
}

/// La revue de release : la réunion détaillée du prototype.
pub fn revue_de_release(id: u32) -> Reunion {
    let duration = hms(2, 34, 22);
    let mut speakers = vec![
        sp("SPEAKER_00", "Jérôme", "Khalid l'appelle « Jérôme » à 0:44:00 et 1:41:02. Mène la revue côté métier.", 35.0, false),
        sp("SPEAKER_01", "Alexandre", "Jérôme l'appelle « Alexandre » à 1:04:46. Présente les bookmarks à 1:58:45.", 255.0, false),
        sp("SPEAKER_02", "Jérôme", "Prolonge presque toujours les répliques de SPEAKER_00 : sans doute la même personne, mal séparée.", 80.0, false),
        sp("SPEAKER_03", "Khalid", "Anime la revue et prend les retours. Appelé « Khalid » à 1:07:04.", 160.0, false),
        sp("SPEAKER_04", "Pablo", "Nommé par Khalid à 0:05:17. Parle des permissions et du Search.", 320.0, false),
    ];
    speakers[2].fusion_proposee = Some(0);
    let keys = vec![
        u(0, 1.0, 7.0, "Ça ne veut rien dire, « in your portfolio ». Moi je suis sur la prod, sur un compte au hasard."),
        u(0, 17.0, 13.0, "Il y a écrit « Status active in your portfolio ». Je ne sais pas si c'est nouveau, mais ça me paraît mislibé."),
        u(1, 31.0, 6.0, "Tu pourrais nous faire un partage d'écran ?"),
        u(3, 92.0, 18.0, "C'est un fix temporaire : on avait du rouge à un endroit et du vert à un autre, les clients le remontaient."),
        u(3, 243.0, 20.0, "Le Sonar confondait une marque française, EA Sheets, avec un homonyme colombien."),
        u(3, 287.0, 9.0, "On lui donne maintenant le nom de domaine, et c'est déjà push dans le trottoir."),
        u(3, 317.0, 10.0, "Pablo, tu veux expliquer pour le Search ?"),
        u(4, 328.0, 26.0, "Le Search ouvrait des marques hors de la wave du client, donc sans SRS. On l'a fermé à la wave pour l'instant."),
        u(0, 629.0, 22.0, "Au prix où il paie, j'estime que si Bentley signe avec le PSG, je dois avoir Bentley dedans quelques jours après."),
        u(0, 770.0, 18.0, "Eux ils veulent se comparer à la Formule 1, évidemment. Pas à la Formule AI, ils s'en fichent."),
        u(0, 1489.0, 24.0, "Tu es Bruno, tu trouves Whoop pour le maillot à pois, tu signes : tu dois le voir dans ton portfolio dès la start date."),
        u(3, 1530.0, 20.0, "C'est déjà le cas : Global Data saisit le deal, Volcan passe et la brand page apparaît, avec un SRS grisé."),
        u(3, 2640.0, 14.0, "Donc pour clarifier, Jérôme : on vend au moins dix mille marques, et l'Explore peut aller au-delà."),
        u(0, 2656.0, 12.0, "Voilà. Au moins dix mille. C'est ça qu'on écrit dans le contrat."),
        u(4, 2713.0, 22.0, "Le dataset d'un client, ce serait sa wave plus les sponsors actifs des ayants droit qu'il a achetés, en ajout seulement."),
        u(0, 3772.0, 16.0, "Accéder à une marque par l'URL ou le brand ID alors qu'on n'y a pas droit, c'est un bug critique."),
        u(0, 3886.0, 10.0, "Alexandre, c'est toi qui as fait les cartes ?"),
        u(1, 3897.0, 12.0, "Oui, et la HoverCard aussi. Je peux montrer après la release."),
        u(3, 5132.0, 20.0, "Le filtre fiscal year : on a la donnée pour à peu près 70 % des sociétés cotées."),
        u(0, 5493.0, 14.0, "Mets un badge Alpha dessus. Et l'objectif c'est 95 % des cotées."),
        u(3, 6018.0, 16.0, "Petits correctifs maintenant : le libellé du deal sheet, la carte de Market, les tooltips natifs."),
        u(0, 6441.0, 10.0, "Moi je mettrais « Selected Brent », avec le même style que « You »."),
        u(1, 7125.0, 28.0, "Je vous montre les bookmarks : on peut créer ses propres labels, et les retrouver comme colonne dans l'Explore."),
        u(0, 7158.0, 16.0, "Le principe des labels, j'aime bien. Mais là c'est trop chargé, il y a trop de boutons."),
        u(0, 7504.0, 14.0, "Rien ne part chez un client sans que je l'aie validé, on est d'accord ?"),
        u(1, 8324.0, 18.0, "Je peux faire trois versions : une avec le statut Contacted à part, une allégée, une avec des Saved Filters."),
        u(0, 8384.0, 12.0, "Parfait. On les fait tester à des bêta-testeurs avant d'aller voir le client."),
        u(3, 8878.0, 14.0, "Je fais le récap avec toutes les actions et je l'envoie lundi."),
        u(2, 9134.0, 8.0, "Et tu as une date, Samuel, pour la prochaine release ?"),
        u(0, 9230.0, 10.0, "J'attends le compte rendu. Merci à tous."),
    ];
    let mut utterances = weave(keys, duration, &[0.34, 0.16, 0.1, 0.28, 0.12], 7);
    let mut doubts = vec![
        doubt("mislibé", Some("mislabelé"), "« Je ne sais pas si c'est nouveau ou quoi, mais ça me paraît mislibé. »"),
        doubt("EA Sheets", None, "« C'est ce qui s'est passé avec une marque EA Sheets, qui est une boîte française. »"),
        doubt("push dans le trottoir", Some("push en prod"), "« Et ça c'est déjà push dans le trottoir. »"),
        doubt("Formule AI", Some("Formule E"), "« Je ne veux pas me comparer à la Formule AI, je m'en fous. »"),
        doubt("Selected Brent", Some("Selected brand"), "« Moi je mettrais Selected Brent, avec le même style que You. »"),
        doubt("une date, Samuel", None, "« Et tu as une date, Samuel, pour la prochaine release ? »"),
    ];
    attach_doubts(&mut utterances, &mut doubts);
    Reunion {
        locuteurs: speakers,
        repliques: utterances,
        passages: doubts,
        reperes: vec![Repere { instant: hms(1, 58, 45), libelle: "Ma démo bookmarks".into() }],
        corrections: 147,
        ..terminee(id, DateHeure::new(2026, 11, 20, 9, 39), duration, "Revue de release : permissions, fiscal year, bookmarks")
    }
    .avec_bilan(RELEASE_BILAN)
}

pub struct Brief<'a> {
    pub title: &'a str,
    pub brief: &'a str,
    pub topics: &'a [(&'a str, &'a [&'a str])],
    pub decisions: &'a [&'a str],
    pub actions: &'a [(&'a str, &'a str)],
}

/// Les locuteurs des réunions courtes : (étiquette, nom, justification, teinte).
const EQUIPE: [(&str, &str, &str, f32); 4] = [
    ("SPEAKER_00", "Khalid", "Anime le point et distribue les sujets.", 160.0),
    ("SPEAKER_01", "Alexandre", "Parle de ses tickets en cours.", 255.0),
    ("SPEAKER_02", "Pablo", "Fait la review du code.", 320.0),
    ("SPEAKER_03", "Aref", "Produit et data.", 200.0),
];

/// Le début de chaque sujet d'une réunion courte.
fn debuts_des_sujets(b: &Brief, duration: f64) -> Vec<f64> {
    let n = b.topics.len().max(1) as f64;
    (0..b.topics.len()).map(|i| 1.0 + duration * i as f64 / n).collect()
}

/// Les répliques d'une réunion courte : une par sujet, tissées de répliques de remplissage.
fn repliques_generiques(b: &Brief, duration: f64, seed: u64) -> Vec<Replique> {
    let starts = debuts_des_sujets(b, duration);
    let keys: Vec<Replique> = b
        .topics
        .iter()
        .zip(&starts)
        .enumerate()
        .map(|(i, ((t, _), at))| u(i % 4, at + 2.0, 14.0, &format!("Sujet suivant : {}.", t.to_lowercase())))
        .collect();
    weave(keys, duration, &[0.35, 0.25, 0.25, 0.15], seed)
}

/// Une réunion courte et générique (daily, point produit), entièrement traitée.
pub fn generique(id: u32, debut: DateHeure, duration: f64, b: &Brief, seed: u64) -> Reunion {
    Reunion {
        locuteurs: EQUIPE.iter().map(|(l, n, j, h)| sp(l, n, j, *h, true)).collect(),
        repliques: repliques_generiques(b, duration, seed),
        corrections: 30 + (seed % 40) as u32,
        ..terminee(id, debut, duration, b.title)
    }
    .avec_bilan(brief_markdown(b, duration, &debuts_des_sujets(b, duration)))
}

/// Le bilan qu'écrirait l'agent pour une réunion courte, au format du prompt du bilan.
fn brief_markdown(b: &Brief, duration: f64, starts: &[f64]) -> String {
    let ts = |t: f64| {
        let t = t as u64;
        format!("({}:{:02}:{:02})", t / 3600, (t / 60) % 60, t % 60)
    };
    let mut md = format!("## En bref\n\n{}\n\n## Participants\n\n", b.brief);
    md += "- **SPEAKER_00 — Khalid** : anime le point.\n- **SPEAKER_01 — Alexandre** : ses tickets en cours.\n- **SPEAKER_02 — Pablo** : review du code.\n- **SPEAKER_03 — Aref** : produit et data.\n\n## Sujets abordés\n\n";
    for ((t, points), at) in b.topics.iter().zip(starts) {
        md += &format!("### {t} {}\n\n", ts(*at));
        for p in *points {
            md += &format!("- {p}\n");
        }
        md += "\n";
    }
    md += "## Décisions\n\n";
    let nd = b.decisions.len() as f64 + 0.5;
    for (i, d) in b.decisions.iter().enumerate() {
        md += &format!("- {} {d}\n", ts(duration * (i as f64 + 0.6) / nd));
    }
    md += "\n## Actions\n\n";
    for (w, a) in b.actions {
        md += &format!("- [ ] **{w}** — {a}\n");
    }
    md += "\n## Questions ouvertes\n\n- Rien d'ouvert de notable.\n";
    md
}

/// Une réunion enregistrée mais pas encore traitée.
pub fn a_traiter(id: u32, debut: DateHeure, duree: f64) -> Reunion {
    Reunion::new(id, dossier::nom_de_dossier(debut, "Réunion"), debut, duree)
}

/// Ce que les faux adaptateurs inventent pour toute nouvelle réunion.
pub const NOUVELLE_REUNION: Brief = Brief {
    title: "Point produit : maquettes bookmarks, staging",
    brief: "Alexandre a montré les trois maquettes des bookmarks. La version allégée est retenue pour les bêta-testeurs. Pablo signale que staging sera indisponible mardi matin. Aref veut revoir la colonne labels dans l'Explore.",
    topics: &[
        ("Maquettes bookmarks", &["Trois versions présentées.", "La version allégée est retenue."]),
        ("Staging", &["Indisponible mardi matin pour la migration."]),
        ("Colonne labels dans l'Explore", &["Aref veut un tri par label."]),
    ],
    decisions: &["La version allégée des bookmarks part aux bêta-testeurs.", "Pas de déploiement sur staging mardi matin."],
    actions: &[("Alexandre", "Envoyer la version allégée aux bêta-testeurs"), ("Aref", "Spécifier le tri par label dans l'Explore"), ("Pablo", "Prévenir l'équipe de la coupure de staging")],
};

/// Les réunions du prototype, les plus récentes d'abord.
pub fn reunions() -> Vec<Reunion> {
    let m = |h, mi| hms(h, mi, 0);
    let le = |jour, h, mi| DateHeure::new(2026, 11, jour, h, mi);
    let mut v = vec![a_traiter(1, le(21, 11, 2), m(0, 38)), revue_de_release(2)];
    let briefs: [(DateHeure, f64, Brief); 9] = [
        (le(19, 9, 32), m(1, 29), Brief {
            title: "Daily : fiscal year, HoverCard, permissions v2",
            brief: "Tour des tickets. Alexandre finit la HoverCard. Pablo démarre les permissions v2. Khalid prépare la revue de release de vendredi.",
            topics: &[("HoverCard", &["Remplace les tooltips natifs."]), ("Permissions v2", &["Démarrage cette semaine."]), ("Revue de release", &["Vendredi avec Jérôme."])],
            decisions: &["La revue de release se fait vendredi matin."],
            actions: &[("Alexandre", "Finir la HoverCard avant vendredi"), ("Khalid", "Préparer les slides de la revue")],
        }),
        (le(18, 9, 34), m(1, 26), Brief {
            title: "Daily : bookmarks, Crustdata, logos",
            brief: "Point sur l'essai Crustdata pour les logos et sur les bookmarks Axe-One.",
            topics: &[("Logos via Crustdata", &["Couverture correcte, qualité inégale."]), ("Bookmarks Axe-One", &["Prototype validé par Samuel."])],
            decisions: &["On garde Crustdata en secours pour les logos."],
            actions: &[("Alexandre", "Comparer Crustdata et la source actuelle sur 100 logos")],
        }),
        (le(17, 9, 32), m(0, 50), Brief {
            title: "Daily : colonnes de l'Explore",
            brief: "Revue rapide des nouvelles colonnes de l'Explore.",
            topics: &[("Colonnes de l'Explore", &["Ordre par défaut à revoir."])],
            decisions: &["Ordre des colonnes : nom, catégorie, deals actifs."],
            actions: &[("Alexandre", "Changer l'ordre par défaut des colonnes")],
        }),
        (le(16, 14, 0), m(1, 45), Brief {
            title: "Atelier : encyclopédie des règles produit",
            brief: "Atelier pour lister les règles produit implicites et décider où les écrire.",
            topics: &[("Règles implicites", &["Une trentaine identifiées."]), ("Où les écrire", &["Une page par règle dans docs/."])],
            decisions: &["Une page par règle dans le dépôt docs."],
            actions: &[("Alexandre", "Rédiger les dix premières règles"), ("Aref", "Relire les règles data")],
        }),
        // Dimanche, pour rester dans « Semaine dernière » de la barre latérale.
        (le(15, 9, 32), m(1, 32), Brief {
            title: "Daily : sponsorships manquants, banc Sportload",
            brief: "Analyse du banc d'essai sur 83 marques Sportload : 12 sponsorships manquants.",
            topics: &[("Banc d'essai Sportload", &["83 marques, 12 sponsorships manquants."])],
            decisions: &["On corrige les 12 à la main cette semaine."],
            actions: &[("Alexandre", "Remonter les 12 sponsorships manquants à Global Data")],
        }),
        (le(13, 9, 30), m(1, 26), Brief {
            title: "Daily : bandeau KPI, staging",
            brief: "Le bandeau KPI de la brand page est corrigé sur staging.",
            topics: &[("Bandeau KPI", &["Corrigé sur staging."])],
            decisions: &["Le fix du bandeau part en prod lundi."],
            actions: &[("Pablo", "Relire la PR du bandeau")],
        }),
        (le(12, 9, 33), m(1, 6), Brief {
            title: "Daily : logos, explore better columns",
            brief: "Point sur FAL-1515 et sur les logos.",
            topics: &[("FAL-1515", &["Maquette validée."]), ("Logos", &["Essai Crustdata à lancer."])],
            decisions: &["Lancer l'essai Crustdata."],
            actions: &[("Alexandre", "Lancer l'essai Crustdata sur les logos")],
        }),
        (le(11, 9, 30), m(1, 32), Brief {
            title: "Daily : sponsorships manquants",
            brief: "Démarrage du banc d'essai sur les marques Sportload.",
            topics: &[("Banc d'essai", &["Liste des 83 marques prête."])],
            decisions: &["Banc d'essai sur 83 marques."],
            actions: &[("Alexandre", "Construire le banc d'essai")],
        }),
        (le(10, 9, 31), m(0, 24), Brief {
            title: "Daily : nom de l'équipe au survol du logo",
            brief: "FAL-1539 : afficher le nom de l'équipe au survol du logo.",
            topics: &[("FAL-1539", &["Tooltip au survol du logo."])],
            decisions: &["Utiliser la HoverCard plutôt qu'un tooltip natif."],
            actions: &[("Alexandre", "Implémenter FAL-1539")],
        }),
    ];
    for (i, (debut, dur, b)) in briefs.iter().enumerate() {
        v.push(generique(10 + i as u32, *debut, *dur, b, 100 + i as u64));
    }
    v
}

/// Les mots et les tours de parole d'une nouvelle réunion inventée : chaque réplique de
/// [`NOUVELLE_REUNION`] devient des mots contigus et un tour de parole.
fn nouvelle_transcription(duree: f64, graine: u64) -> (Vec<Mot>, Vec<TourDeParole>) {
    let mut mots = Vec::new();
    let mut tours = Vec::new();
    for r in repliques_generiques(&NOUVELLE_REUNION, duree, graine) {
        let morceaux: Vec<&str> = r.texte.split(' ').collect();
        let n = morceaux.len() as f64;
        let a = |i: usize| r.debut + (r.fin - r.debut) * i as f64 / n;
        for (i, m) in morceaux.iter().enumerate() {
            let fin = if i + 1 == morceaux.len() { r.fin } else { a(i + 1) };
            mots.push(Mot { debut: a(i), fin, texte: (*m).to_string(), confiance: 0.9, locuteur: None });
        }
        tours.push(TourDeParole { debut: r.debut, fin: r.fin, locuteur: EQUIPE[r.locuteur].0.into() });
    }
    // Toute l'équipe est détectée, même qui ne parle presque pas.
    for (l, ..) in EQUIPE {
        if !tours.iter().any(|t| t.locuteur == l) {
            tours.push(TourDeParole { debut: duree, fin: duree, locuteur: l.into() });
        }
    }
    (mots, tours)
}

/// Le bilan de l'agent fictif : le bilan actuel quand on le refait, sinon celui de
/// [`NOUVELLE_REUNION`].
fn bilan_invente(d: &DemandeDeBilan) -> Result<(String, ResultatDuBilan), String> {
    let session = Some("session-fictive".to_string());
    if let Some(md) = &d.bilan_actuel {
        let participants = d
            .locuteurs
            .iter()
            .filter_map(|l| Some(Participant { locuteur: l.etiquette.clone(), nom_propose: l.proposition.clone()?, justification: l.justification.clone(), meme_voix_que: l.meme_voix_proposee.clone() }))
            .collect();
        let resultat = ResultatDuBilan { session, titre: d.titre.clone(), resume: bilan::en_bref(md), actions: bilan::actions(md).into_iter().take(3).collect(), participants };
        return Ok((md.clone(), resultat));
    }
    let b = &NOUVELLE_REUNION;
    let md = brief_markdown(b, d.duree, &debuts_des_sujets(b, d.duree));
    let resultat = ResultatDuBilan {
        session,
        titre: b.title.into(),
        resume: b.brief.into(),
        actions: b.actions.iter().take(3).map(|(qui, quoi)| format!("{qui} — {quoi}")).collect(),
        participants: EQUIPE.iter().map(|(l, n, j, _)| Participant { locuteur: (*l).into(), nom_propose: (*n).into(), justification: (*j).into(), meme_voix_que: None }).collect(),
    };
    Ok((md, resultat))
}

/// Les faux adaptateurs du prototype.
pub fn adaptateurs() -> Adaptateurs {
    // Le transcripteur et le séparateur inventent la même réunion : même graine, appel après appel.
    let (mut graine_t, mut graine_s) = (100, 100);
    Adaptateurs {
        capture: Box::new(FausseCapture::new(VITESSE_D_ENREGISTREMENT, 60.0)),
        transcripteur: Box::new(FauxTranscripteur::new(duree_simulee(Etape::Transcription), move |d| {
            graine_t += 1;
            Ok(nouvelle_transcription(d.duree, graine_t - 1).0)
        })),
        separateur: Box::new(FauxSeparateur::new(duree_simulee(Etape::Transcription), move |d| {
            graine_s += 1;
            Ok(nouvelle_transcription(d.duree, graine_s - 1).1)
        })),
        agent: Box::new(FauxAgent::new(duree_simulee(Etape::Relecture), |_| Ok(ResultatDeRelecture::default()), duree_simulee(Etape::Bilan), bilan_invente)),
        ressources: Box::new(FaussesRessources::fixes(ressources())),
        horloge: Box::new(FausseHorloge(AUJOURD_HUI)),
        corbeille: Box::new(FausseCorbeille::new()),
    }
}

fn ressource(nom: &str, detail: &str, etat: EtatDeRessource, taille: u64, total: u64, etapes: &[Etape]) -> Ressource {
    Ressource { nom: nom.into(), detail: detail.into(), etat, taille, total, aide: None, etapes: etapes.to_vec() }
}

/// Les ressources d'une installation complète, aux tailles réelles.
pub fn ressources() -> Vec<Ressource> {
    let t = [Etape::Transcription];
    vec![
        ressource("Whisper large-v3", "huggingface.co/ggerganov/whisper.cpp", EtatDeRessource::Prete, 3_095_033_483, 3_095_033_483, &t),
        ressource("Silero VAD v5.1.2", "huggingface.co/ggml-org/whisper-vad", EtatDeRessource::Prete, 885_098, 885_098, &t),
        ressource("pyannote community-1", "release GitHub de Greffier · CC-BY-4.0", EtatDeRessource::Prete, 32_955_421, 32_955_421, &t),
        ressource("Python privé", "pyannote.audio 4.0.7 · installé par uv", EtatDeRessource::Prete, 1_184_312_904, 0, &t),
        ressource("Claude Code", "2.1.329 · connectée (claude.ai)", EtatDeRessource::Prete, 0, 0, &[Etape::Relecture, Etape::Bilan]),
    ]
}

/// Un premier lancement en cours : whisper se télécharge, le Python s'installe, le sha256 d'un
/// fichier de pyannote n'a pas correspondu et la CLI `claude` n'est pas installée.
pub fn ressources_en_installation() -> Vec<Ressource> {
    let mut r = ressources();
    r[0].etat = EtatDeRessource::EnTelechargement { recu: 1_342_177_280, total: 3_095_033_483 };
    r[0].taille = 1_342_177_280;
    r[2].etat = EtatDeRessource::Echec("sha256 incorrect pour modeles/pyannote-community-1/embedding/pytorch_model.bin. Le fichier est supprimé ; il sera retéléchargé au prochain essai.".into());
    r[2].taille = 6_174_803;
    r[3].etat = EtatDeRessource::EnInstallation;
    r[3].taille = 0;
    r[4].detail = "CLI d'Anthropic".into();
    r[4].etat = EtatDeRessource::Absente;
    r[4].aide = Some("La relecture et le bilan passent par Claude Code. L'installer (https://claude.com/claude-code), lancer `claude` une fois pour se connecter, puis cliquer sur « Vérifier à nouveau ».".into());
    r
}

/// Le bilan de la revue de release, comme l'agent l'écrit avec le prompt du bilan.
const RELEASE_BILAN: &str = r#"## En bref

Khalid a présenté à Jérôme les correctifs de la release. La plus grosse partie de la réunion a porté sur le cas Hisense remonté par Sportload : le Search, les URL et les brand IDs donnent accès à des marques hors du dataset du client. Décision : on vend **« au moins 10 000 marques »**, et le dataset d'un client devient sa wave plus les sponsors actifs des ayants droit achetés, en ajout seulement. Le filtre fiscal year reçoit un badge Alpha. Les labels des bookmarks plaisent mais l'interface est trop chargée : trois versions iront aux bêta-testeurs.

## Participants

- **SPEAKER_00 — Jérôme** : mène la revue côté métier. Khalid l'appelle « Jérôme » (0:44:00).
- **SPEAKER_01 — Alexandre** : front, cartes, HoverCard, bookmarks. Présente les bookmarks (1:58:45).
- **SPEAKER_02 — sans doute Jérôme aussi** : erreur probable de séparation des locuteurs.
- **SPEAKER_03 — Khalid** : anime la revue de release et prend les retours.
- **SPEAKER_04 — Pablo** : permissions, Search, SRS.

## Sujets abordés

### Bandeau du portfolio (0:00:01)

- Libellé « Status active in your portfolio » jugé trompeur.
- Fix temporaire en attendant le redesign du bandeau et du KPI d'annualized value.

### Sonar : homonyme (0:03:46)

- Une marque française confondue avec un homonyme colombien.
- Correctif : donner au Sonar le nom de domaine. Bouton *Restart Analysis* réservé aux admins.

### Search fermé à la wave (0:05:17)

- Sportload ouvrait des marques hors de sa wave, donc sans SRS.
- Pour Jérôme, le coût du SRS est le seul vrai frein. Exemple de Bentley qui signe avec le PSG.

### Ce que voit le client quand un deal est signé (0:24:49)

- Speedrun de Jérôme avec Whoop.
- Global Data saisit le deal, Volcan exécute la pipeline, SRS grisé hors wave.

### Périmètre : au moins 10 000 marques (0:44:00)

- On vend « au moins 10 000 marques ».
- Dataset = wave + sponsors actifs des ayants droit achetés, en ajout seulement.

### Accès hors permissions (1:02:30)

- URL et brand ID ouvrent des marques hors dataset : bug critique.

### Filtre fiscal year (1:25:32)

| Mesure | Aujourd'hui | Objectif |
|---|---:|---:|
| Couverture des sociétés cotées | 70 % | 95 % |

### Petits correctifs (1:40:18)

- Libellé « Selected brand », même style que « You ».
- Remplacer les tooltips natifs par la HoverCard.

### Bookmarks et labels (1:58:31)

- Labels personnalisés validés.
- Interface trop chargée : trois versions pour les bêta-testeurs.

### Bêta-test et process de release (2:27:58)

- Process proposé : prototype → bêta → staging → prod.

## Décisions

- (0:44:12) On vend **« au moins 10 000 marques »** : l'Explore peut dépasser le volume contractuel.
- (0:45:13) Dataset d'un client = sa wave + les sponsors actifs des ayants droit achetés, en ajout seulement.
- (1:02:52) L'accès à des marques hors permissions (Search, URL, brand ID) est un bug critique.
- (1:31:33) Badge **Alpha** sur le filtre fiscal year, objectif 95 % des sociétés cotées.
- (1:59:04) Rien n'est montré à un client sans validation de Jérôme.
- (2:18:44) Bookmarks / labels et statut « Contacted » restent deux systèmes distincts.

## Actions

- [ ] **Alexandre** — préparer trois versions de l'UI bookmarks / labels pour les bêta-testeurs
- [ ] **Alexandre** — ajouter le badge Alpha au filtre fiscal year
- [ ] **Alexandre** — passer le libellé à « Selected brand », même style que « You »
- [ ] **Pablo** — permissions v2 : dataset additif et fermeture de l'accès hors permissions
- [ ] **Khalid** — envoyer le récap de la réunion avec toutes les actions — lundi 23/11
- [ ] **Jérôme** — relancer la liste de bêta-testeurs

## Questions ouvertes

- Calendrier des permissions v2.
- Coût du SRS sur les nouvelles marques, par client et par mois.
- Modèle LFP / Ligue 1 : à trancher avec Vincent.
"#;
