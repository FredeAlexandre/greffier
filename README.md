# Greffier

App Mac qui enregistre les réunions de travail, les transcrit, les relit et en fait le bilan, pour
retrouver vite ce qui a été dit, décidé et à faire. Tout tourne sur le Mac (capture, transcription,
séparation des locuteurs) ; seules la relecture et le bilan passent par un agent Claude Code. Les
réunions sont rangées en fichiers lisibles (Markdown, JSON, vidéo) dans un dossier de réunions, pour
l'utilisateur comme pour ses agents.

## État

**Cycle 1 en cours.** L'interface existe, écrite avec [Rune](https://github.com/anomalyco/rune), au-dessus
d'un cœur sans interface ; le cœur tourne sur des données fictives et de faux adaptateurs : capture réelle, import, Whisper, pyannote,
Claude et écriture des fichiers restent à brancher. Pendant les cycles, l'app ne vise que le Mac de
son auteur ; la 1.0.0 viendra avec un guide d'installation et des paquets téléchargeables.

- User stories, cycles et critères de la 1.0.0 : [docs/user-stories.md](docs/user-stories.md)
- Vocabulaire du domaine : [GLOSSARY.md](GLOSSARY.md)
- Décisions d'architecture : [docs/adr/](docs/adr/)

## Lancer

```sh
cargo run            # la fenêtre (première compilation : ~2 min)
cargo test           # le cœur, sans fenêtre, sur ses faux adaptateurs
```

## Installer l'app

```sh
scripts/installer-app
```

Compile en release, construit `Greffier.app` (identifiant `io.github.fredealexandre.greffier`,
version lue dans `Cargo.toml`, descriptions d'usage de l'écran, du micro et du son du système,
runtime durci avec l'entitlement micro), la signe avec l'identité locale « Greffier Dev », la
remplace dans `/Applications` (en fermant l'app si elle tourne) puis l'ouvre et attend sa fenêtre.

- La première fois, l'identité « Greffier Dev » (certificat autosigné de signature de code, 10 ans)
  est créée dans le trousseau de session ; ensuite elle est réutilisée. Si le trousseau est
  verrouillé, la commande s'arrête et dit de lancer `security unlock-keychain`, qui demande le mot
  de passe de session.
- macOS rattache les autorisations (écran, micro, son du système, notifications) à l'identifiant et
  au certificat : elles tiennent d'une installation à l'autre. Ne supprimez pas l'identité et ne
  changez pas l'identifiant, sinon il faut les redonner.
- `--binaire CHEMIN` empaquette un exécutable déjà compilé, `--sans-lancer` n'ouvre pas l'app.
- Le bundle embarque `uv` (version et sha256 figés dans `scripts/installer_app.py`), téléchargé une
  fois dans `target/uv-<version>/`, où `cargo run` le trouve aussi.
- Tests de l'empaquetage : `python3 -m unittest discover -s scripts`.

## Ressources du premier lancement

Au premier lancement, la fenêtre affiche l'état de chaque ressource et les installe toutes dans
`~/Library/Application Support/Greffier/`, et nulle part ailleurs :

- whisper large-v3 et Silero VAD v5.1.2, depuis Hugging Face ; pyannote `speaker-diarization-community-1`
  (CC-BY-4.0), depuis la release [`pyannote-community-1`](https://github.com/FredeAlexandre/greffier/releases/tag/pyannote-community-1)
  du dépôt, publiée avec sa licence et son attribution par `scripts/publier-pyannote`. Chaque fichier
  est vérifié par un sha256 figé ; un téléchargement interrompu reprend où il s'est arrêté.
- le Python privé de pyannote, installé par l'uv embarqué à partir de `coeur/ressources/python/uv.lock`.
- la CLI `claude` est seulement vérifiée (installée et connectée), avec quoi faire sinon.
- le lexique livré (`coeur/ressources/lexique.md`) y est écrit s'il n'existe pas encore.

Sans fenêtre : `cargo run -p greffier-coeur --example installer_ressources` (`-- --verifier` pour ne
faire que les vérifications). Les scènes et parcours gardent de fausses ressources.

## À essayer

| Geste | Où |
|---|---|
| ⇧⌘R ou le bouton rouge | démarre / arrête un enregistrement (temps accéléré ×20) |
| ⌘M pendant l'enregistrement | pose un repère, visible ensuite sur la timeline |
| Arrêter | la réunion arrive, passe par Transcription → Relecture → Bilan, bandeau « Bilan prêt » |
| « Réunion du sam. 21/11 » | réunion pas encore traitée : nombre de locuteurs, « Transcrire et faire le bilan » |
| « Revue de release » | vidéo pleine largeur, timeline par sujet et par locuteur, décisions (◆), repères |
| Espace, ← → | lecture / pause, ±10 s |
| Glisser sur les pistes | déplace la lecture ; clic sur un sujet ou un ◆ : y saute |
| ⌘1 … ⌘5 | Bilan / Transcription / À vérifier / Locuteurs / Infos |
| Bilan | Markdown rendu ; les ▶ horodatages lancent la lecture, les cases se cochent, « Refaire le bilan » |
| ⌘K ou « Rechercher » | cherche dans les titres, bilans et transcriptions ; ↩ ouvre au bon moment |
| Clic droit sur une réunion | menu : ouvrir, lire la transcription, refaire le bilan, afficher dans le Finder, copier le bilan, supprimer (corbeille du Mac, après confirmation) |

## Sans fenêtre

Les scènes et parcours scriptés servent de tests visuels :

```sh
target/debug/greffier --list-scenes
target/debug/greffier --snap out.png --scene meeting --theme dark
target/debug/greffier --script scripts/parcours-enregistrement.json --snap fin.png --at 20
```

## Code

- `coeur/` : le cœur, sans interface (crate `greffier-coeur`, sans Rune). Il possède l'état des
  réunions, de l'enregistrement et de la file de traitement ; on le pilote par commandes
  (`Greffier::executer`) et on fait passer le temps avec `Greffier::avancer`.
  - `adaptateurs.rs` : les interfaces de tout ce qui sort du processus (capture, transcripteur,
    séparateur de locuteurs, agent, ressources, horloge) ; `faux.rs` : leurs faux.
  - `reunion.rs` : réunion, statut, locuteurs, répliques, passages à vérifier ; `bilan.rs` : ce
    qu'on lit dans le bilan Markdown ; `dossier.rs` : le nommage du dossier de réunions.
  - `ressources.rs` : le catalogue des ressources (URL, tailles, sha256) et l'adaptateur de
    ressources de l'app ; `mac.rs` : ses vrais outils (curl, sha256, uv embarqué, CLI `claude`).
  - `lexique.rs` et `ressources/lexique.md` : le lexique livré et sa section Vocabulaire.
  - `fictif.rs` : les données fictives et les faux adaptateurs du prototype.
  - `tests/` : le banc de test (dossier temporaire, faux adaptateurs, commandes, état et fichiers).
- `src/main.rs` : l'interface autour du cœur : lecture, raccourcis, shell, scènes.
- `src/meeting.rs` : page réunion, vidéo, onglets. `src/timeline.rs` : la timeline. `src/overlays.rs` : recherche ⌘K, confirmation de suppression.
- `src/pages.rs` : vue d'ensemble, enregistrement, réglages, premier lancement. `src/sidebar.rs` : barre latérale et menu du clic droit.
- `scripts/installer_app.py` : empaquetage, signature et installation de `Greffier.app`.
- `scripts/publier-pyannote` : publication de la release des fichiers pyannote (`scripts/pyannote/` : licence, attribution).

Rune est épinglé sur une révision de `dev` (pré-release) dans `Cargo.toml` ; on ne la change que
volontairement ([ADR 0004](docs/adr/0004-interface-en-rune.md)).
