# Review and summary handed to Claude Code as an agent, not to the API

The review and the summary call the `claude` CLI installed on the Mac (`claude -p`, opus model, resumable sessions), as the vault does today, instead of the Claude API. These stages are agent tasks: Claude can reason, reread previous summaries, look up data and use tools, and we don't want to reduce that to "here is the transcription, write the summary". It is the only dependency Greffier does not bundle: the app checks that `claude` is installed and signed in, and does not replace it.
