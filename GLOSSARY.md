# Greffier

Greffier enregistre les réunions de travail sur le Mac, les transcrit, les relit et en fait le bilan, pour qu'on puisse les revoir et retrouver ce qui a été dit, décidé et à faire.

## Réunion et fichiers

**Réunion** :
Une conversation enregistrée, avec tout ce qui en est tiré : enregistrement, transcription, relecture, bilan.
_À éviter_ : meeting, session, call

**Dossier de réunions** :
Le dossier choisi par l'utilisateur où Greffier range toutes les réunions, un sous-dossier par réunion, en fichiers lisibles hors de l'app.
_À éviter_ : bibliothèque, vault, base

**Fiche** :
Le fichier d'entrée d'une réunion : ses propriétés (date, durée, statut…), son résumé, ses actions principales et les liens vers les autres fichiers.
_À éviter_ : note de réunion, page

**Titre** :
Le résumé en une ligne d'une réunion (« Revue de release »), proposé par le bilan et modifiable.
_À éviter_ : sujet, nom

**Enregistrement** :
La capture vidéo et audio d'une réunion (écran, son du Mac, micro).
_À éviter_ : recording, vidéo (quand on parle du fichier complet)

**Repère** :
Un instant marqué par l'utilisateur pendant l'enregistrement, pour le retrouver ensuite sur la timeline.
_À éviter_ : marqueur, bookmark

**Import** :
L'ajout d'une réunion à partir de fichiers existants (vidéo ou audio seul, avec ou sans transcription, avec ou sans bilan) ; le traitement reprend à la première étape manquante.
_À éviter_ : migration

## Traitement

**Traitement** :
La chaîne d'étapes qui transforme un enregistrement en transcription relue et en bilan.
_À éviter_ : pipeline, job

**Étape** :
Un maillon du traitement : Transcription, Relecture ou Bilan.

**Transcription** :
Le texte de la réunion découpé en répliques horodatées et attribuées à des locuteurs ; aussi l'étape qui la produit.
_À éviter_ : sous-titres, verbatim

**Réplique** :
Un passage continu d'un même locuteur dans la transcription, avec son horodatage de début et de fin.
_À éviter_ : segment, ligne

**Locuteur** :
Une voix distincte détectée dans une réunion (SPEAKER_00, SPEAKER_01…), à laquelle on peut donner un nom. Deux locuteurs peuvent se révéler être la même voix et être fusionnés.
_À éviter_ : speaker, personne, participant

**Nombre de locuteurs** :
L'indication donnée avant le traitement du nombre de voix attendues, pour mieux séparer les locuteurs ; « détecté » si on ne la donne pas.
_À éviter_ : personnes, participants

**Lexique** :
La liste des noms propres, mots du métier et erreurs de transcription connues avec leur forme correcte, qui sert à la transcription et à la relecture et s'enrichit à chaque réunion.
_À éviter_ : dictionnaire, glossaire

## Relecture et correction

**Relecture** :
L'étape automatique où une IA corrige les erreurs sûres de la transcription, change les locuteurs quand la conversation le prouve et signale les passages douteux.
_À éviter_ : relecture IA (sauf dans l'interface), révision, proofreading

**Réécoute** :
La retranscription d'un passage douteux, isolé de l'enregistrement, pour que la relecture tranche.

**Passage à vérifier** :
Un passage que la relecture n'a pas su trancher, même après réécoute, et que l'utilisateur doit écouter.
_À éviter_ : doute, passage surligné

**Correction** :
Une modification faite à la main par l'utilisateur sur la transcription ou les locuteurs.
_À éviter_ : relecture, édition

## Bilan

**Bilan** :
La synthèse rédigée d'une réunion : en bref, locuteurs nommés, sujets abordés, décisions, actions et questions ouvertes, avec horodatages.
_À éviter_ : résumé, compte rendu, notes

**Sujet** :
Un point abordé pendant la réunion, horodaté à son début dans le bilan ; les sujets forment les chapitres de la timeline.
_À éviter_ : thème, chapitre, titre

**Décision** :
Un choix acté pendant la réunion, consigné et horodaté dans le bilan.

**Action** :
Une chose à faire issue de la réunion, sous la forme « qui — quoi — échéance », cochable dans le bilan.
_À éviter_ : tâche, todo
