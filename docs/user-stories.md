# Greffier — user stories

> Une app Mac qui enregistre les réunions de travail, les transcrit, les relit et en fait le bilan,
> pour retrouver vite ce qui a été dit, décidé et à faire. Vocabulaire : [GLOSSARY.md](../GLOSSARY.md).
> Décisions d'architecture : [docs/adr/](adr/).
>
> **Périmètre** : Greffier couvre enregistrement → transcription → relecture → bilan → revoir → retrouver.
> Ce n'est pas un gestionnaire de tâches : les décisions et actions vivent dans le bilan de chaque réunion.

## Cycles et versions

Jusqu'à la première vraie release, le travail avance par **cycles** (Cycle 1, Cycle 2…). Le numéro de
version du logiciel n'en dépend pas : la **1.0.0** sortira quand l'app satisfera Alexandre et que les
critères de [Avant la 1.0.0](#avant-la-100) seront remplis. Ensuite, on passe au versionnage.

**Pendant les cycles**, Greffier doit marcher sur **le Mac d'Alexandre et lui seul** (Apple M5, 24 Go,
macOS 27). Aucune exigence de portabilité, mais rien qui la rende plus difficile : pas de chemin propre
à cette machine écrit en dur, modèles téléchargés depuis des adresses publiques.

Chaque story porte son cycle et son origine :
- **parité** : le vault Obsidian le fait aujourd'hui (sous une autre forme) ;
- **nouveau** : ni le vault ni ses outils ne le font.

## D'où on part : le process du vault

Greffier remplace le process « Réunions » du vault Falcon HQ (`~/Vault/Falcon HQ`, outils
`Outils/Enregistreur` et `Outils/Réunions`). **Aucune compatibilité n'est due à ce vault** : ni ses
formats de fichiers, ni sa structure de notes, ni ses outils. On n'en reprend que l'expérience et la
**qualité du traitement** (modèles, paramètres, étapes) — voir [Le traitement repris du vault](#le-traitement-repris-du-vault).

| Étape | Aujourd'hui dans le vault | Ce qui change avec Greffier |
|---|---|---|
| Enregistrer | Raccourci Raycast → `record-meeting` pilote OBS (écran, son du Mac, micro ; MP4 H.264, 3 pistes audio). | Capture native, sans OBS ni Raycast. |
| Indiquer le nombre de locuteurs | Champ `personnes` de la fiche. | Champ dans la réunion avant le traitement. |
| Transcrire | Bouton Meta Bind → `reunion traiter --fond` : ffmpeg, whisper.cpp large-v3 et pyannote en parallèle, attribution des mots aux locuteurs. | Même pipeline, embarqué ; rien installé dans l'environnement de l'utilisateur. |
| Relecture | `claude -p` en deux passes avec réécoute des passages douteux ; le lexique s'enrichit. | Identique, via la CLI `claude`. |
| Bilan | `claude -p` avec `prompt-summary.md` ; l'agent écrit le bilan lui-même. | Identique. |
| Suivre | Encadré « État » dans la fiche Markdown, notification, file d'attente. | Dans l'interface. |
| Revoir | Plugin Media One : vidéo et transcription cliquable. | Lecteur, timeline par sujet et par locuteur, bilan cliquable. |
| Retrouver | `Index des réunions.md` régénéré par code. | Liste, vue d'ensemble, recherche ⌘K. |
| Entretenir | `doctor.ts` vérifie OBS, ffmpeg, whisper.cpp, modèles, pyannote, Claude Code. | Vérification au premier lancement et dans les réglages. |

Les frictions à faire disparaître : OBS, Raycast et Homebrew à installer et configurer ; deux outils
séparés ; l'état d'avancement lu dans une fiche Markdown ; la vidéo dans un plugin ; les locuteurs
jamais renommés (les noms n'existent que dans le bilan) ; les passages ==surlignés== à retrouver dans
un long fichier.

## Personas

- **Alexandre** (utilisateur principal) : dev, plusieurs réunions par jour (daily, revues de release,
  points produit), veut retrouver vite « ce qui a été décidé » et « ce que je dois faire ».
- **Un agent IA** (utilisateur secondaire) : lit les bilans et transcriptions sur la machine pour
  préparer une tâche. Les fichiers doivent être lisibles et explicites hors de l'app.

## Les fichiers d'une réunion

Les fichiers font foi ([ADR 0001](adr/0001-fichiers-ouverts-dans-un-meetings-folder.md)). Chaque
réunion est un dossier du **dossier de réunions** ; chaque fichier reprend la date et le titre, pour
pouvoir être partagé seul sans être renommé :

```
<dossier de réunions>/
└── 2026-11-19 09h32 - Revue de release/
    ├── 2026-11-19 09h32 - Revue de release - réunion.md
    ├── 2026-11-19 09h32 - Revue de release - bilan.md
    ├── 2026-11-19 09h32 - Revue de release - transcription.md
    ├── 2026-11-19 09h32 - Revue de release - transcription.json
    └── 2026-11-19 09h32 - Revue de release - enregistrement.mp4
```

| Fichier | Auteur | Contenu |
|---|---|---|
| `réunion.md` (la **fiche**) | Greffier | Frontmatter : titre, date, début, durée, statut, nombre de locuteurs, noms des locuteurs (`SPEAKER_00 → Nom`), repères, sessions Claude. Corps : résumé, 3 actions principales, liens vers les autres fichiers. |
| `bilan.md` | l'agent Claude | Le bilan, jamais réécrit par Greffier. |
| `transcription.md` | Greffier | Répliques horodatées avec le **nom** des locuteurs ; passages à vérifier marqués. |
| `transcription.json` | Greffier | Mots, horodatages, confiances, locuteurs, transcription brute d'avant relecture, journal de la relecture, passages à vérifier. |
| `enregistrement.mp4` | Greffier | Vidéo H.264 et 3 pistes audio (mixage, micro seul, son du Mac seul). |

- Avant le bilan, le titre vaut « Réunion ». Quand le titre change, Greffier renomme le dossier et
  tous ses fichiers et met à jour les liens entre eux ; les caractères interdits deviennent des tirets.
- Rien d'autre à la racine du dossier de réunions : pas d'index, pas de LISEZMOI.
- Greffier ne garde qu'un cache reconstructible (recherche) dans `~/Library/Application Support/Greffier/`.
  Il surveille le dossier : un fichier modifié par l'utilisateur ou un agent est rechargé, un dossier
  supprimé dans le Finder fait disparaître la réunion.

### Ce que Greffier range ailleurs

Tout dans `~/Library/Application Support/Greffier/`, rien dans `~/.cache` ni dans l'environnement :
- `models/` : whisper large-v3, Silero VAD, pyannote `community-1` ;
- `runtime/` : le Python privé de pyannote ([ADR 0003](adr/0003-pyannote-dans-un-python-prive.md)) ;
- `lexique.md` : le lexique d'Alexandre, rempli au départ à partir de celui du vault ; il contient aussi
  la section **Vocabulaire** donnée à Whisper ;
- le cache de recherche.

Les prompts de relecture et de bilan sont livrés avec l'app et versionnés avec le code.

## Epics et stories

### E1. Enregistrer une réunion

- **US-1.1 · Cycle 1 · parité** — En tant qu'Alexandre, je démarre et j'arrête un enregistrement d'un
  clic (bouton de l'app ou de la barre des menus) ou d'un raccourci global, sans OBS ni Raycast.
  - Critères : un seul raccourci démarre et arrête ; l'app capture l'écran, le son du Mac et le micro,
    au format du vault (H.264 1080p 30 i/s à 1200 kbps, AAC 160 kbps, 3 pistes : mixage, micro seul,
    son du Mac seul) ; une icône reste visible dans la barre des menus pendant toute la durée.
- **US-1.2 · Cycle 1 · nouveau** — Pendant l'enregistrement, je vois la durée, les niveaux du micro et du
  son du Mac, et les sources actives, pour savoir que ça enregistre bien.
- **US-1.3 · Cycle 1 · nouveau** — Pendant l'enregistrement, je pose un **repère** d'un raccourci (⌘M),
  pour retrouver ce moment sur la timeline.
- **US-1.4 · Cycle 1 · parité** — À l'arrêt, la réunion apparaît dans la liste avec sa date et sa durée ;
  le traitement démarre seul si le réglage est activé.
- **US-1.5 · Cycle 1 · parité** — Un enregistrement de moins d'une minute est proposé à la suppression
  plutôt que rangé.
- **US-1.6 · Cycle 1 · nouveau** — Si l'app ou le Mac plante pendant un enregistrement, je ne perds pas
  ce qui a été capturé.
  - Critères : l'enregistrement est écrit au fil de l'eau dans le dossier de la réunion, dans un format
    lisible même s'il est interrompu ; au relancement, la réunion apparaît marquée « enregistrement
    interrompu » et peut être traitée ; le Mac ne se met pas en veille pendant un enregistrement.
- **US-1.7 · Cycle 1 · parité** — J'**importe** une réunion passée par glisser-déposer ou par un menu :
  un média seul, un média et sa transcription, ou un média, sa transcription et son bilan.
  - Critères : média = tout ce qu'AVFoundation lit (mp4, mov, m4a, wav, mp3…), vidéo ou audio seul ;
    transcription = `transcription.json` de Greffier, WebVTT ou SRT, avec les locuteurs reconnus en tête
    de réplique (`<v Nom>`, `[Nom]:` ou `Nom :`) ; bilan = n'importe quel `.md`, conservé tel quel.
  - La date vient du média (date de création, sinon de modification), modifiable à l'import ; le titre
    vient du bilan s'il y en a un, sinon du nom du fichier.
  - Le traitement reprend à la première étape manquante : média seul → Transcription, Relecture, Bilan ;
    avec transcription → Relecture, Bilan ; avec bilan → rien. Une transcription importée est
    considérée comme non relue ; sans locuteurs, elle est attribuée à un seul locuteur.
  - À la fin de l'import, une fenêtre demande de lancer le traitement maintenant ou plus tard.
  - Les réunions du vault s'importent ainsi (vidéo, `.vtt` au format `[SPEAKER_00]: …`, bilan), sans
    code propre au vault ; on accepte de perdre les horodatages au mot et le journal de relecture.
- **US-1.8 · Cycle 2 · nouveau** — L'app me propose d'enregistrer quand une réunion de mon agenda
  commence ou quand Meet / Zoom ouvre le micro.
- **US-1.9 · Cycle 2 · nouveau** — Je choisis audio seul ou audio et écran, pour économiser le disque
  (une réunion de 89 min fait environ 1,1 Go en vidéo).

### E2. Traiter : transcription, relecture, bilan

- **US-2.1 · Cycle 1 · parité** — Avant le traitement, j'indique le **nombre de locuteurs** (ou je laisse
  « détecté »), parce que la séparation des locuteurs est bien meilleure avec le bon nombre.
- **US-2.2 · Cycle 1 · parité** — Je vois l'avancement par étape (Transcription → Relecture → Bilan), avec
  une progression et une estimation du temps restant, dans la liste et dans la réunion.
- **US-2.3 · Cycle 1 · parité** — Les réunions à traiter attendent dans une file, une à la fois ; je vois
  leur rang et je peux arrêter un traitement.
  - Critères : la file se met en pause pendant un enregistrement et reprend après ; une étape déjà en
    cours au début de l'enregistrement va jusqu'au bout ; le Mac ne se met pas en veille pendant un
    traitement.
- **US-2.4 · Cycle 1 · parité** — Une notification macOS m'avertit quand le bilan est prêt ; un clic
  ouvre la réunion.
- **US-2.5 · Cycle 1 · parité** — En cas d'erreur, l'étape en échec est marquée avec le message et un
  bouton « Réessayer cette étape ».
- **US-2.6 · Cycle 1 · parité** — Je peux refaire n'importe quelle étape et celles qui suivent : refaire
  le bilan seul (après avoir nommé des locuteurs ou corrigé la transcription), refaire depuis la
  relecture, ou refaire la transcription avec un autre nombre de locuteurs.
- **US-2.7 · Cycle 1 · parité** — La relecture et le bilan sont confiés à un **agent** Claude Code, qui
  peut raisonner, relire les bilans précédents et chercher du contexte, et pas seulement résumer la
  transcription ([ADR 0002](adr/0002-relecture-et-bilan-par-claude-code.md)).
  - Critères : mêmes outils autorisés qu'aujourd'hui (relecture : lecture seule ; bilan : lecture et
    écriture), Bash et web interdits ; l'agent part du dossier de réunions et a accès aux **dossiers de
    contexte** des réglages ; l'agent écrit `bilan.md` et renvoie titre, résumé et 3 actions pour la
    fiche ; Greffier vérifie que le bilan contient les sujets horodatés, les décisions et les actions,
    sinon l'étape est en échec.
- **US-2.8 · Cycle 2 · nouveau** — L'app me dit clairement ce qui reste sur le Mac (transcription,
  locuteurs) et ce qui part vers Claude (relecture, bilan).

### E3. Corriger : locuteurs et passages à vérifier

- **US-3.1 · Cycle 1 · nouveau** — Je vois les locuteurs détectés (SPEAKER_00…) avec le nom que l'IA
  propose et sa justification (« Khalid l'appelle Jérôme à 0:44:00 »), et je confirme ou corrige en un
  clic.
  - Critères : nommer un locuteur met à jour la fiche et `transcription.md` ; on peut fusionner deux
    locuteurs (une même voix sous deux labels, erreur fréquente de la séparation des locuteurs).
  - L'onglet « Personnes » du prototype s'appelle « Locuteurs ».
- **US-3.2 · Cycle 1 · nouveau** — Je parcours un par un les **passages à vérifier** (25 sur la dernière
  revue de release), avec le texte, l'hypothèse de l'IA et la réécoute ; j'écoute l'extrait, j'accepte
  l'hypothèse, je garde l'original ou je tape la correction.
- **US-3.3 · Cycle 1 · nouveau** — Je corrige un mot directement dans la transcription.
- **US-3.4 · Cycle 2 · nouveau** — Je réattribue une réplique à un autre locuteur.
- **US-3.5 · Cycle 2 · nouveau** — Mes corrections nourrissent le lexique.
- **US-3.6 · Cycle 2 · nouveau** — Greffier reconnaît ma voix grâce à la piste micro seul et nomme
  « Alexandre » le bon locuteur sans passer par l'IA.

### E4. Revoir une réunion

- **US-4.1 · Cycle 1 · parité** — J'ouvre une réunion et je vois d'abord le **bilan** : titre, en bref,
  sujets, décisions, actions, questions ouvertes.
  - Le bilan est du Markdown écrit par l'agent. L'app le rend stylé sans le restructurer et n'en lit que
    ce dont l'interface a besoin : les sujets `### Titre (h:mm:ss)` deviennent les chapitres de la
    timeline, les puces horodatées de « Décisions » ses ◆, chaque `(h:mm:ss)` un lien qui lance la
    lecture. Les cases `- [ ]` se cochent dans le fichier.
- **US-4.2 · Cycle 1 · nouveau** — La vidéo prend toute la largeur, les contrôles sont juste en dessous,
  et la réplique en cours s'affiche en sous-titre.
- **US-4.3 · Cycle 1 · nouveau** — Une **timeline** sous le lecteur montre qui parle quand (une piste par
  locuteur), les sujets comme chapitres, les décisions et mes repères ; cliquer n'importe où y déplace
  la lecture.
- **US-4.4 · Cycle 1 · parité** — La transcription défile avec la lecture, la réplique en cours est mise
  en avant ; cliquer une réplique y saute.
- **US-4.5 · Cycle 1 · nouveau** — Chaque horodatage du bilan (sujet, décision) est cliquable et lance la
  lecture à ce moment, pour vérifier une citation.
- **US-4.6 · Cycle 1 · nouveau** — Je règle la vitesse de lecture (1×, 1,5×, 2×) et j'avance ou recule de
  10 s au clavier.
- **US-4.7 · Cycle 1 · nouveau** — Une réunion importée ou ancienne à laquelle il manque des données
  (pas de timeline, pas de relecture…) s'affiche quand même, avec ce qui existe.
- **US-4.8 · Cycle 2 · nouveau** — Je cherche un mot dans la transcription d'une réunion et les
  occurrences s'affichent sur la timeline.
- **US-4.9 · Cycle 2 · parité** — Je reprends la conversation du bilan avec Claude
  (`claude --resume`) pour poser une question sur la réunion.

### E5. Retrouver

- **US-5.1 · Cycle 1 · parité** — La barre latérale liste les réunions par période (Aujourd'hui, Cette
  semaine, Plus tôt), avec titre, heure, durée et état du traitement.
- **US-5.2 · Cycle 1 · parité** — Une vue d'ensemble montre le total (27 réunions, 29 h 37), le temps
  passé en réunion par jour et les réunions en attente.
- **US-5.3 · Cycle 1 · nouveau** — Une recherche (⌘K) parcourt titres, bilans et transcriptions de toutes
  les réunions ; un résultat ouvre la réunion au bon moment (la réplique, ou le sujet pour une ligne du
  bilan).
- **US-5.4 · Cycle 1 · nouveau** — Clic droit sur une réunion : Ouvrir, Lire la transcription, Refaire le
  bilan, Afficher dans le Finder, Copier le bilan, **Supprimer** (avec confirmation : le dossier de la
  réunion va à la corbeille).
- **US-5.5 · Cycle 1 · nouveau** — Je change le titre d'une réunion ; le dossier et ses fichiers sont
  renommés.
- **US-5.6 · Cycle 2 · nouveau** — Je filtre par locuteur nommé (« toutes les réunions avec Jérôme ») et
  par sujet.
- **US-5.7 · Cycle 2 · nouveau** — Je donne des étiquettes à une réunion.

### E6. Des fichiers ouverts

- **US-6.1 · Cycle 1 · nouveau** — Greffier range chaque réunion dans le dossier de réunions, en fichiers
  au nom explicite (voir [Les fichiers d'une réunion](#les-fichiers-dune-réunion)), pour que moi et mes
  agents puissions les lire, les partager et les exploiter sans l'app.
  - Critères : chaque fichier se comprend seul (date et titre dans le nom) ; une modification faite
    hors de l'app est prise en compte.
- **US-6.2 · Cycle 2 · nouveau** — Je copie un bilan ou une liste d'actions (Markdown ou Slack) d'un clic,
  et j'exporte une transcription en SRT, VTT ou texte.
- **US-6.3 · Cycle 2 · à repenser** — Je crée une tâche à partir d'une action d'un bilan. (Dans le vault,
  c'était une fiche dans `Tâches/` ; Greffier ne doit pas dépendre de cette structure.)

### E7. Réglages et santé

- **US-7.1 · Cycle 1 · parité** — Au premier lancement, l'app me guide avec un état clair par point.
  - Critères : autorisations macOS (écran, micro, son du système) ; choix du dossier de réunions
    (proposé : `~/Documents/Réunions`) ; téléchargement des modèles et du Python privé dans
    `~/Library/Application Support/Greffier/`, avec progression et vérification sha256 ; présence et
    connexion de la CLI `claude`, la seule dépendance non embarquée.
- **US-7.2 · Cycle 1 · parité** — Je règle le dossier de réunions, les dossiers de contexte de l'agent, le
  raccourci, les sources et le démarrage automatique du traitement ; je vois la place prise par les
  modèles et j'ouvre le lexique dans mon éditeur.
- **US-7.3 · Cycle 2 · nouveau** — J'édite le lexique et les prompts de relecture et de bilan depuis l'app.

### E8. Installer (pendant les cycles)

- **US-8.1 · Cycle 1 · nouveau** — En tant qu'Alexandre développeur, une commande construit
  `Greffier.app`, la signe avec une identité stable (« Greffier Dev ») et l'installe dans
  `/Applications`, pour que les autorisations macOS survivent aux recompilations.

## Le traitement repris du vault

La qualité vient du vault ; les paramètres sont repris à l'identique (référence :
`~/Vault/Falcon HQ/Outils/Réunions/src/`). Pas de choix de modèle dans l'app.

1. **Audio** : piste de mixage, convertie en wav mono 16 kHz PCM 16 bits, par AVFoundation au lieu de
   ffmpeg — à condition de ne rien perdre sur les extraits A, B et C de `~/Bench-Transcription`
   (taux d'erreur, termes clés, erreurs de locuteur, durée) ; sinon ffmpeg est embarqué en binaire
   statique.
2. **Transcription** (en parallèle de 3) : whisper.cpp large-v3 (`ggml-large-v3.bin`, sha256 figé), sur
   Metal, `-l fr -t 6`, vocabulaire du lexique en prompt avec `--carry-initial-prompt`, un segment par
   mot (`-ml 1 -sow`, sortie JSON complète), VAD Silero v5.1.2, valeurs par défaut de whisper-cli pour
   le reste (beam 5, température 0 avec repli 0,2). Les bruits entre crochets sont retirés.
3. **Séparation des locuteurs** : pyannote.audio 4.0.7, pipeline `speaker-diarization-community-1`, hors
   ligne, sur MPS avec repli CPU, sortie « exclusive », nombre de locuteurs si indiqué. Dépendances
   figées par le `uv.lock` du vault.
4. **Attribution des mots** : mots décalés de +0,2 s, chacun au locuteur qui chevauche le plus ;
   interruptions de 2 mots ou moins rendues au locuteur en cours ; changement de locuteur recalé sur la
   fin de phrase à 3 mots près ; nouvelle réplique à chaque changement de locuteur, pause de 2 s, ou fin
   de phrase après 220 caractères.
5. **Relecture** : `claude -p` modèle opus, deux passes dans la même session. Passe 1 : corrections
   sûres, changements de locuteur prouvés, passages douteux (30 au plus), ajouts au lexique. Réécoute de
   chaque passage douteux (±1,5 s, 28 s au plus) avec la même transcription. Passe 2 : chaque passage est
   corrigé ou reste à vérifier.
6. **Bilan** : `claude -p` modèle opus avec le prompt du bilan ; sections En bref, Participants (chaque
   locuteur nommé ou décrit), Sujets abordés horodatés, Décisions horodatées, Actions
   (`- [ ] Qui — quoi — échéance`, celles d'Alexandre d'abord), Questions ouvertes.

Mesure de référence (20/11, réunion de 2 h 34) : transcription 36 min, relecture 6 min, bilan 2 min.

Le banc d'essai place large-v3-turbo à égalité avec large-v3 pour 25 % de temps en moins ; on garde
large-v3 tant que les références du banc ne sont pas validées.

## Avant la 1.0.0

Critères de sortie de la période des cycles, à traiter dans un cycle ultérieur :

- **Installation depuis les sources** : un guide court (quelques commandes), avec un fichier de
  configuration mise ou nix pour l'outillage ; le choix se fera au moment d'écrire le guide.
- **Release GitHub** : une GitHub Action publie à chaque tag un `.dmg` qu'on installe d'un double-clic,
  sans alerte de Gatekeeper, donc signé Developer ID et notarisé par Apple (compte Apple Developer à
  prendre à ce moment-là), avec sommes sha256.
- **Macs visés** : idéalement n'importe quel Mac. À évaluer : ce qui est possible sur les Mac Intel et
  les petites cartes graphiques (whisper large-v3 et pyannote sans Metal ni MPS), et la version minimale
  de macOS (ScreenCaptureKit, Rune).
- **Redistribution des modèles** : pyannote `community-1` (CC-BY-4.0) publié en release du dépôt avec un
  fichier `ATTRIBUTION` ; whisper.cpp et Silero sous MIT.

## Ce que montre le prototype

Le prototype (`src/`) est **l'interface seule**, avec des données fictives et des traitements simulés.
La vraie app se construit dessus en remplaçant peu à peu le faux par le vrai ; ses scènes sans fenêtre
(`--snap`, `--script`) servent de tests visuels.

1. **Vue d'ensemble** (US-5.2) : totaux, dernières réunions, temps en réunion par jour.
2. **Enregistrer** (US-1.1 → 1.4) : bouton dans la barre latérale ou ⇧⌘R, durée, niveaux, sources,
   repères (⌘M), puis Arrêter.
3. **Traitement simulé** (US-2.1 → 2.4) : Transcription → Relecture → Bilan, visible dans la liste et
   dans la réunion (accéléré).
4. **Réunion** (US-4.1 → 4.6) : vidéo pleine largeur, timeline par sujet et par locuteur, onglets
   Bilan / Transcription / À vérifier / Personnes (à renommer Locuteurs) / Infos.
5. **Bilan en Markdown** (US-4.1) : rendu stylé, horodatages cliquables, cases cochables, « Refaire le
   bilan ».
6. **Locuteurs** (US-3.1) : nom proposé par l'IA, confirmer, changer, fusionner.
7. **À vérifier** (US-3.2) : écouter, remplacer par l'hypothèse de l'IA, garder l'original.
8. **Recherche ⌘K** (US-5.3) et **clic droit / Supprimer** (US-5.4).

Hors prototype : capture réelle, import, Whisper, pyannote, Claude, écriture des fichiers.
