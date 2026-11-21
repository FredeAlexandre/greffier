# L'interface est écrite avec Rune, épinglé sur une révision de `dev`

Greffier garde Rune (Rust, rendu Metal), déjà utilisé par le prototype, plutôt qu'une app SwiftUI ou web : il couvre nativement le raccourci global, la lecture vidéo et audio via AVFoundation, les notifications, les menus contextuels, le glisser-déposer et le travail en arrière-plan. Rune est en pré-version, ses API changent beaucoup et il n'accepte pas de contributions : on l'épingle sur une révision précise (avec son bloc `[patch.crates-io]`) et on ne la change que volontairement.

## Conséquences

- L'icône de la barre des menus n'existe pas encore sur macOS dans Rune : on l'écrit nous-mêmes en `objc2` (NSStatusItem) depuis le thread principal (`AppHandle::update`).
- La capture écran + son du Mac (ScreenCaptureKit) est aussi à écrire nous-mêmes ; Rune n'enregistre que le micro.
- Rune ne fabrique pas d'app : on a notre propre script de bundle (Info.plist, entitlements, signature).
