# Greffier

App Mac qui enregistre les réunions de travail, les transcrit, les relit et en fait le bilan, pour
retrouver vite ce qui a été dit, décidé et à faire. Tout tourne sur le Mac (capture, transcription,
séparation des locuteurs) ; seules la relecture et le bilan passent par un agent Claude Code. Les
réunions sont rangées en fichiers lisibles (Markdown, JSON, vidéo) dans un dossier de réunions, pour
l'utilisateur comme pour ses agents.

## État

**Cycle 1 en cours.** L'interface existe, écrite avec [Rune](https://github.com/anomalyco/rune), mais
avec des données fictives et des traitements simulés : capture réelle, import, Whisper, pyannote,
Claude et écriture des fichiers restent à brancher. Pendant les cycles, l'app ne vise que le Mac de
son auteur ; la 1.0.0 viendra avec un guide d'installation et des paquets téléchargeables.

- User stories, cycles et critères de la 1.0.0 : [docs/user-stories.md](docs/user-stories.md)
- Vocabulaire du domaine : [GLOSSARY.md](GLOSSARY.md)
- Décisions d'architecture : [docs/adr/](docs/adr/)

## Lancer

```sh
cargo run            # la fenêtre (première compilation : ~2 min)
```

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
| ⌘1 … ⌘5 | Bilan / Transcription / À vérifier / Personnes / Infos |
| Bilan | Markdown rendu ; les ▶ horodatages lancent la lecture, les cases se cochent, « Refaire le bilan » |
| ⌘K ou « Rechercher » | cherche dans les titres, bilans et transcriptions ; ↩ ouvre au bon moment |
| Clic droit sur une réunion | menu : ouvrir, transcription, refaire le bilan, Finder, supprimer (avec confirmation) |

## Sans fenêtre

Les scènes et parcours scriptés servent de tests visuels :

```sh
target/debug/greffier --list-scenes
target/debug/greffier --snap out.png --scene meeting --theme dark
target/debug/greffier --script scripts/parcours-enregistrement.json --snap fin.png --at 20
```

## Code

- `src/main.rs` : état, horloge simulée (lecture, enregistrement, file de traitement), raccourcis, shell.
- `src/data.rs` : modèle (réunion, locuteurs, répliques, passages à vérifier) et données fictives.
- `src/bilan.rs` : le bilan Markdown — sujets et décisions lus dans le bilan, horodatages en liens, cases à cocher.
- `src/meeting.rs` : page réunion, vidéo, onglets. `src/timeline.rs` : la timeline. `src/overlays.rs` : recherche ⌘K, confirmation de suppression.
- `src/pages.rs` : vue d'ensemble, enregistrement, réglages. `src/sidebar.rs` : barre latérale et menu du clic droit.

Rune est épinglé sur une révision de `dev` (pré-release) dans `Cargo.toml` ; on ne la change que
volontairement ([ADR 0004](docs/adr/0004-interface-en-rune.md)).
