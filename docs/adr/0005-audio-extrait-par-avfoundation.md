# L'audio des moteurs est extrait par AVFoundation, pas par ffmpeg

Greffier décode la piste de mixage par AVFoundation et la rééchantillonne lui-même à 16 kHz (`moteurs/src/audio.rs`), au lieu d'embarquer ffmpeg comme le vault. La spec posait une condition : ne rien perdre sur le banc de parité (`scripts/banc-parite`). Le banc du 21/11/2026 compare AVFoundation et ffmpeg sur le même média, avec les mêmes moteurs, sur les trois extraits de 5 minutes (2 479 mots de référence, encore en brouillon).

| Même média, mêmes moteurs | Erreurs de mots | Termes justes | Locuteurs faux |
|---|---:|---:|---:|
| Enregistrement d'origine (AAC d'Apple), AVFoundation | 6,2 % | 61/67 | 2,5 % |
| Enregistrement d'origine, ffmpeg (le WAV du vault) | 6,9 % | 59/67 | 2,6 % |
| m4a ré-encodé par ffmpeg, AVFoundation | 6,3 % | 62/67 | 2,9 % |
| m4a ré-encodé par ffmpeg, ffmpeg | 6,8 % | 62/67 | 2,4 % |
| Vault (whisper-cli, pyannote, assign.ts) | 7,3 % | 59/67 | 2,6 % |

Cumulé sur les deux médias, AVFoundation fait 6,3 % d'erreurs de mots, 123/134 termes et 2,7 % d'erreurs de locuteur, contre 6,8 %, 121/134 et 2,5 % avec ffmpeg. C'est dans les tolérances fixées avant le banc (0,5 point d'erreurs de mots, 1 terme, 0,5 point d'erreurs de locuteur). On garde donc AVFoundation. Sur l'AAC d'Apple, celui des enregistrements de Greffier, les deux décodages sont presque identiques (RMS 0,06 à 0,09 sur 32 768). Ils s'écartent davantage sur l'AAC de ffmpeg (RMS 13 à 22), sans perte mesurable. Sur le WAV du vault, le whisper.cpp intégré rend les mêmes mots, au centième près, que whisper-cli : les extraits B et C sont identiques, et A l'est jusqu'au mot 509, où un écart d'arrondi entre les deux compilations fait diverger la suite.

Ce qu'on accepte : le décodage AAC d'Apple n'est pas déterministe. Deux décodages du même fichier diffèrent d'un bit sur quelques dizaines d'échantillons, et il manque parfois les 3 dernières millisecondes. Whisper étant chaotique, deux transcriptions du même enregistrement peuvent donc différer de quelques mots (de 62 à 64 termes justes d'un passage à l'autre sur le m4a). Si les références validées à l'écoute renversent ces chiffres, on relance le banc et on embarque ffmpeg en binaire statique.
