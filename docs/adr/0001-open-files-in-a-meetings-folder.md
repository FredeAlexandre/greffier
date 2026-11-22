# Les réunions sont des fichiers ouverts dans un dossier de réunions

Greffier n'a pas de base de données propre : chaque réunion est un sous-dossier de fichiers lisibles (Markdown, JSON, vidéo) dans un dossier de réunions choisi dans les réglages, pour que l'utilisateur et ses agents IA puissent les lire et les exploiter sans passer par l'app. On a écarté une bibliothèque opaque avec export, qui aurait coupé les agents de leur source.

Le format de ces fichiers est conçu pour Greffier, pas hérité du vault Obsidian qui l'a précédé : aucune compatibilité n'est due à l'ancien outil, qui n'a servi que d'expérience. Les anciennes réunions entrent par l'import, comme n'importe quel enregistrement externe.
