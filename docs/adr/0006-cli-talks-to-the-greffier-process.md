# The command line reads the folder directly and asks the Greffier process for everything else

`greffier <command>` lets an agent acting for the user do what the UI does, and above all find quickly what was said in a meeting. Reading (`search`, `list`, `show`, `status`) goes straight to the meetings folder and the search cache, without ever launching the app: the files are open (ADR 0001), and an agent must not wait for a process, nor start one, to read them. Everything that writes or processes (import, process, redo, stop, rename, speakers, corrections, deletion) is sent to the Greffier process over a local Unix socket in the app folder (`greffier.sock`, one JSON object per line), which turns each request into a core `Command`. That process stays the only owner of the processing queue and the only writer of the meetings folder, so the command line never races the window on a file and never runs a second queue.

If Greffier is not running, the command line starts it without a window (`open -g -j -b <bundle id> --args --background`, or the binary itself in development): the macOS permissions stay attached to the bundle, the menu bar icon shows the progress and lets the user quit, and opening Greffier during that time brings up the window of the same process, with the processing under way. A process started this way quits on its own after five minutes with an empty queue, if its window was never opened. A lock file in the app folder keeps a single process per app folder; a socket left behind by a dead process is removed.

We rejected a command line that loads the core and processes on its own: two processes would write the same meetings and run two queues, and processing outside the bundle would lose its permissions. We rejected going through the app for reading too: launching a Mac app to read a Markdown file costs seconds and a menu bar icon for nothing.

## Consequences

- One binary, `greffier`: without a command it opens the window as before; with one it acts as the command line, and `cargo xtask install-app` links it into the `PATH`.
- A meeting has a short, stable identifier derived from its folder's date and time (`2026-11-20-0939`), see the glossary.
- `GREFFIER_APP_FOLDER` replaces the app folder for a single launch, as `GREFFIER_FOLDER` replaces the meetings folder: the tests and trials never touch the real ones, and the socket, which lives in the app folder, follows. The command line checks that the process it reaches works on the meetings folder it expects.
- The ⌘K search keeps its limits; the command line uses the same index with its own filters and limits.
