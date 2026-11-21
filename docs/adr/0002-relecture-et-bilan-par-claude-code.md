# Relecture et bilan confiés à Claude Code en agent, pas à l'API

La relecture et le bilan appellent la CLI `claude` installée sur le Mac (`claude -p`, modèle opus, sessions reprenables), comme le vault aujourd'hui, au lieu de l'API Claude. Ces étapes sont des tâches d'agent : Claude peut raisonner, relire les bilans précédents, chercher des données et utiliser des outils, et on ne veut pas réduire ça à « voici la transcription, fais le bilan ». C'est la seule dépendance que Greffier n'embarque pas : l'app vérifie que `claude` est installé et connecté, et ne la remplace pas.
