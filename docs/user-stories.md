# Greffier — user stories

> Nom de code provisoire. Une app Mac qui reprend le process « Réunions » du vault Falcon HQ :
> enregistrer → transcrire → relire → faire le bilan → retrouver.
>
> **Périmètre** : l'app enveloppe seulement enregistrement → transcription → bilan. Elle n'est pas
> un outil de gestion des tâches : pas de vue « mes actions » transverse. Les actions et décisions
> vivent dans le bilan de chaque réunion.

## D'où on part : le process actuel dans le vault

Deux outils du vault, pilotés par des boutons Obsidian (Meta Bind + Shell commands) et un raccourci Raycast :

| Étape | Aujourd'hui | Fichier produit |
|---|---|---|
| 1. Enregistrer | Raccourci Raycast « Enregistrer une réunion » → `record-meeting` pilote OBS (écran + son du Mac + micro, MP4 H.264). Même raccourci pour arrêter. Moins d'une minute : reste dans `~/Movies/Recordings/`. | `Réunions/AAAA-MM-JJ HHhMM - Réunion/… - enregistrement.mp4` + fiche `00 - Réunion …` + index redessiné |
| 2. Renseigner | Champ **Personnes** de la fiche (nombre de locuteurs), sinon détecté. | propriété `personnes` |
| 3. Transcrire | Bouton « Transcrire et faire le bilan » → `reunion traiter --fond`. ffmpeg extrait le son, whisper.cpp large-v3 (horodatage par mot) et pyannote (qui parle quand) tournent en parallèle, puis chaque mot reçoit un locuteur et on regroupe en répliques. ~1 min 30 pour 10 min. | `… - transcription.md`, `… .whisper.json`, `transcription/` (JSON, SRT, VTT, TXT) |
| 4. Relire (IA) | Claude corrige ce qui est sûr (lexique), change les locuteurs quand c'est prouvé, liste les passages douteux ; ffmpeg + Whisper réécoutent ces passages ; Claude tranche ; le reste est ==surligné==. Le lexique s'enrichit. | `… - relecture.md` (corrections, locuteurs changés, « à vérifier ») |
| 5. Bilan (IA) | `claude -p` avec `prompt-bilan.md` : En bref, Participants (SPEAKER_xx → nom), Sujets abordés horodatés, Décisions horodatées, Actions (celles d'Alexandre d'abord), Questions ouvertes. Renvoie sujet + résumé + 3 actions pour la fiche. | `… - bilan.md`, fiche mise à jour |
| 6. Suivre | Encadré « État » de la fiche (Enregistrement / Transcription / Relecture IA / Bilan IA), notification à la fin, une réunion à la fois, file d'attente, bouton Arrêter / Refaire le bilan. | propriétés `etape`, `progression`, `erreur` |
| 7. Revoir | Plugin Media One : vidéo avec transcription cliquable. Fiche avec résumé et actions. | — |
| 8. Retrouver | `Index des réunions` : total, heures, activité par jour, chronologie par semaine. | `Index des réunions.md` |
| 9. Exploiter | Les fiches de tâches citent les réunions en `origines` / `contexte`. Reprendre la conversation du bilan : `claude --resume <thread>`. | liens dans `Tâches/` |
| 10. Entretenir | `doctor.ts` vérifie le Mac (OBS, autorisations, ffmpeg, whisper.cpp, modèles, pyannote, Claude Code, disque). Lexique, prompts de transcription, relecture et bilan éditables. | fiches `Enregistreur`, `Réunions` |

Les points de friction que l'app doit faire disparaître : OBS et Raycast à configurer, deux outils séparés, l'état d'avancement lu dans une fiche Markdown, la vidéo et la transcription dans un plugin, les locuteurs à renommer à la main, les passages ==surlignés== à retrouver dans un long fichier.

## Personas

- **Alexandre** (utilisateur principal) : dev, plusieurs réunions par jour (daily, revues de release, points produit), veut retrouver vite « ce qui a été décidé » et « ce que je dois faire ».
- **Un agent IA** (utilisateur secondaire) : lit les bilans et transcriptions pour préparer une tâche. Les fichiers doivent rester lisibles hors de l'app.

## Epics et stories

Priorité : **V1** = dans le prototype et la première version, **V2** = ensuite.

### E1. Enregistrer une réunion

- **US-1.1 (V1)** — En tant qu'Alexandre, je démarre un enregistrement d'un clic (bouton de l'app ou de la barre des menus) ou d'un raccourci global, pour ne pas dépendre d'OBS ni de Raycast.
  - Critères : un seul raccourci démarre et arrête ; l'app capture l'écran, le son du Mac et le micro ; un indicateur reste visible (barre des menus) pendant toute la durée.
- **US-1.2 (V1)** — Pendant l'enregistrement, je vois la durée, les niveaux du micro et du son du Mac, et les sources actives, pour savoir que ça enregistre bien.
- **US-1.3 (V1)** — Pendant l'enregistrement, je peux poser un **repère** (« moment important ») d'un raccourci, pour le retrouver dans la timeline.
- **US-1.4 (V1)** — À l'arrêt, la réunion apparaît dans la liste avec sa date et sa durée, et le traitement démarre seul (option dans les réglages).
- **US-1.5 (V1)** — Un enregistrement de moins d'une minute est proposé à la suppression plutôt que rangé.
- **US-1.6 (V2)** — Je peux importer une vidéo ou un audio existant (glisser-déposer), comme `record-meeting vault <fichier>`.
- **US-1.7 (V2)** — L'app me propose d'enregistrer quand une réunion de mon agenda commence ou quand Meet / Zoom ouvre le micro.
- **US-1.8 (V2)** — Je choisis audio seul ou audio + écran, pour économiser le disque (une réunion de 2 h 30 fait 1,7 Go en vidéo).

### E2. Traiter : transcription, relecture IA, bilan

- **US-2.1 (V1)** — Avant le traitement, je peux indiquer le nombre de personnes (ou laisser « détecté »), parce que la séparation des locuteurs est bien meilleure avec le bon nombre.
- **US-2.2 (V1)** — Je vois l'avancement par étape (Transcription → Relecture IA → Bilan), avec une progression et une estimation du temps restant, dans la liste et dans la réunion.
- **US-2.3 (V1)** — Les réunions à traiter attendent dans une file, une à la fois ; je vois leur rang et je peux arrêter un traitement.
- **US-2.4 (V1)** — Une notification macOS m'avertit quand le bilan est prêt ; un clic ouvre la réunion.
- **US-2.5 (V1)** — En cas d'erreur, l'étape en échec est marquée avec le message et un bouton « Réessayer cette étape ».
- **US-2.6 (V1)** — Je peux refaire le bilan seul (après avoir renommé des locuteurs ou corrigé la transcription), sans retranscrire. Le bouton est dans l'onglet Bilan.
- **US-2.7 (V2)** — Je peux refaire la transcription avec un autre nombre de personnes.
- **US-2.8 (V2)** — Transcription et locuteurs tournent sur le Mac ; seuls la relecture et le bilan partent vers Claude, et l'app le dit clairement.

### E3. Corriger : locuteurs et passages douteux

- **US-3.1 (V1)** — Je vois les locuteurs détectés (SPEAKER_00…) avec le nom que l'IA propose et sa justification (« Khalid l'appelle Jérôme à 0:44:00 »), et je confirme ou corrige en un clic.
  - Critères : renommer met à jour toute la transcription ; on peut fusionner deux locuteurs (erreur de diarisation fréquente : une personne sous deux labels).
- **US-3.2 (V1)** — Je vois la liste des passages « à vérifier » (25 sur la dernière revue de release), avec le texte, l'hypothèse de l'IA et la réécoute ; je les parcours un par un, j'écoute l'extrait, j'accepte l'hypothèse, je garde l'original ou je tape la correction.
- **US-3.3 (V1)** — Je peux corriger un mot directement dans la transcription.
- **US-3.4 (V2)** — Je peux réattribuer une réplique à un autre locuteur.
- **US-3.5 (V2)** — Mes corrections nourrissent le lexique (noms, mots du métier, erreurs connues), que je peux consulter et éditer.

### E4. Revoir une réunion

- **US-4.1 (V1)** — J'ouvre une réunion et je vois d'abord le **bilan** : sujet, en bref, décisions, actions, questions ouvertes.
  - Le bilan est **du Markdown** que le LLM écrit en remplissant un template (`prompt-bilan.md`). L'app le rend stylé, sans le restructurer. Elle en lit seulement ce dont l'interface a besoin : les sujets `### Titre (h:mm:ss)` deviennent les chapitres de la timeline, les puces horodatées de « Décisions » ses ◆, et chaque `(h:mm:ss)` devient un lien qui lance la lecture. Les cases `- [ ]` se cochent dans le Markdown.
- **US-4.0 (V1)** — La vidéo prend toute la largeur, les contrôles sont juste en dessous, et les sous-titres de la réplique en cours s'affichent dessus.
- **US-4.2 (V1)** — Une **timeline** sous le lecteur montre qui parle quand (une piste par locuteur), les sujets abordés comme chapitres, les décisions et mes repères ; cliquer n'importe où y déplace la lecture.
- **US-4.3 (V1)** — La **transcription** défile avec la lecture, la réplique en cours est mise en avant ; cliquer une réplique y saute.
- **US-4.4 (V1)** — Chaque horodatage du bilan (sujet, décision) est cliquable et lance la lecture à ce moment, pour vérifier une citation.
- **US-4.5 (V1)** — Je règle la vitesse de lecture (1×, 1,5×, 2×) et j'avance / recule de 10 s au clavier.
- **US-4.6 (V2)** — Je cherche un mot dans la transcription d'une réunion et les occurrences s'affichent sur la timeline.
- **US-4.7 (V2)** — Je peux « reprendre la conversation » du bilan avec Claude pour poser une question sur la réunion.

### E5. Retrouver

- **US-5.1 (V1)** — La barre latérale liste les réunions par période (Aujourd'hui, Cette semaine, Plus tôt), avec sujet, heure, durée et état du traitement.
- **US-5.2 (V1)** — Une vue d'ensemble montre le total (27 réunions, 29 h 37), le temps passé en réunion par jour et les réunions en attente, comme l'index actuel.
- **US-5.3 (V1)** — Une recherche (⌘K) parcourt sujets, bilans et transcriptions de toutes les réunions. Un résultat ouvre la réunion au bon moment (l'heure de la réplique, ou celle du sujet pour une ligne du bilan).
- **US-5.4 (V1)** — Clic droit sur une réunion de la liste : Ouvrir, Lire la transcription, Refaire le bilan, Afficher dans le Finder, Copier le bilan, **Supprimer** (avec confirmation : vidéo, transcription et bilan vont à la corbeille).
- **US-5.5 (V2)** — Je filtre par personne (« toutes les réunions avec Jérôme ») et par sujet.
- **US-5.6 (V2)** — Je renomme une réunion et je lui donne des étiquettes.

### E6. Garder le vault comme source

- **US-6.1 (V1)** — L'app range tout dans le dossier `Réunions/` du vault, au même format qu'aujourd'hui (fiche, transcription, relecture, bilan en Markdown, vidéo), pour que les agents et Obsidian continuent de les lire.
- **US-6.2 (V2)** — Je copie un bilan ou une liste d'actions (Markdown ou Slack) d'un clic.
- **US-6.3 (V2)** — Je crée une fiche de tâche depuis une action, avec la réunion en `origines`.

### E7. Réglages et santé

- **US-7.1 (V1)** — Au premier lancement, l'app demande les autorisations macOS (écran, micro, son du système) et vérifie les modèles, comme `doctor.ts`, avec un état clair par point.
- **US-7.2 (V1)** — Je choisis le dossier du vault, le raccourci, les sources et le démarrage automatique du traitement.
- **US-7.3 (V2)** — J'édite le lexique et les prompts (transcription, relecture, bilan) depuis l'app.

## Ce que montre le prototype

Le prototype (`src/`) est **l'interface seule**, avec des données fictives et des traitements simulés. Il sert à cliquer et ressentir le parcours :

1. **Vue d'ensemble** (US-5.2) : totaux, dernières réunions, temps en réunion par jour.
2. **Enregistrer** (US-1.1 → 1.4) : bouton dans la barre latérale ou ⇧⌘R, durée, niveaux, sources, repères (⌘M), puis Arrêter.
3. **Traitement simulé** (US-2.1 → 2.4) : Transcription → Relecture IA → Bilan, visible dans la liste et dans la réunion (accéléré).
4. **Réunion** (US-4.0 → 4.5) : vidéo pleine largeur, timeline par sujet et par locuteur, onglets Bilan / Transcription / À vérifier / Personnes / Infos. Pas de panneau à droite.
5. **Bilan en Markdown** (US-4.1) : rendu stylé, horodatages cliquables, cases cochables, « Refaire le bilan » dans l'onglet.
6. **Personnes** (US-3.1) : nom proposé par l'IA, confirmer, changer, fusionner.
7. **À vérifier** (US-3.2) : écouter, remplacer par l'hypothèse de l'IA, garder l'original.
8. **Recherche ⌘K** (US-5.3) et **clic droit / Supprimer** (US-5.4).

Hors prototype : capture réelle, Whisper, pyannote, Claude, écriture dans le vault.
