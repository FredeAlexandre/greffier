# Greffier — prototype d'interface

App Mac qui reprend le process « Réunions » du vault Falcon HQ : enregistrer → transcrire →
relire → faire le bilan → retrouver. **Interface seule**, avec des données fictives et des
traitements simulés, écrite avec [Rune](https://github.com/anomalyco/rune).

- User stories et analyse du process actuel : [docs/user-stories.md](docs/user-stories.md)

## Lancer

```sh
cargo run            # la fenêtre (première compilation : ~2 min)
```

## À essayer

| Geste | Où |
|---|---|
| ⇧⌘R ou le bouton rouge | démarre / arrête un enregistrement (temps accéléré ×20) |
| ⌘M pendant l'enregistrement | pose un repère, visible ensuite sur la timeline |
| Arrêter | la réunion arrive, passe par Transcription → Relecture IA → Bilan, bandeau « Bilan prêt » |
| « Réunion du sam. 21/11 » | réunion pas encore traitée : nombre de personnes, « Transcrire et faire le bilan » |
| « Revue de release » | vidéo pleine largeur, timeline par sujet et par locuteur, décisions (◆), repères |
| Espace, ← → | lecture / pause, ±10 s |
| Glisser sur les pistes | déplace la lecture ; clic sur un sujet ou un ◆ : y saute |
| ⌘1 … ⌘5 | Bilan / Transcription / À vérifier / Personnes / Infos |
| Bilan | Markdown rendu ; les ▶ horodatages lancent la lecture, les cases se cochent, « Refaire le bilan » |
| ⌘K ou « Rechercher » | cherche dans les titres, bilans et transcriptions ; ↩ ouvre au bon moment |
| Clic droit sur une réunion | menu : ouvrir, transcription, refaire le bilan, Finder, supprimer (avec confirmation) |

## Sans fenêtre

```sh
target/debug/greffier --list-scenes
target/debug/greffier --snap out.png --scene meeting --theme dark
target/debug/greffier --script scripts/parcours-enregistrement.json --snap fin.png --at 20
```

## Code

- `src/main.rs` : état, horloge simulée (lecture, enregistrement, file de traitement), raccourcis, shell.
- `src/data.rs` : modèle (réunion, locuteurs, répliques, passages douteux) et données fictives.
- `src/bilan.rs` : le bilan Markdown — sujets et décisions lus dans le template, horodatages en liens, cases à cocher.
- `src/meeting.rs` : page réunion, vidéo, onglets. `src/timeline.rs` : la timeline. `src/overlays.rs` : recherche ⌘K, confirmation de suppression.
- `src/pages.rs` : vue d'ensemble, enregistrement, réglages. `src/sidebar.rs` : barre latérale et menu du clic droit.

Rune est épinglé sur une révision de `dev` (pré-release) dans `Cargo.toml`.
