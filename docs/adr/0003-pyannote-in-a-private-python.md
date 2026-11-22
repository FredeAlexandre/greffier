# Diarization runs in a private Python, managed by the app

So that the results are identical to the vault's, Greffier reuses the same pipeline (whisper.cpp large-v3 and pyannote `speaker-diarization-community-1`) instead of a native port (ONNX / sherpa-onnx). pyannote only exists in Python: the app installs a Python and its pinned dependencies (`uv.lock`) itself in `~/Library/Application Support/Greffier/`, next to the downloaded models, without touching the user's environment. A native port remains possible later, once compared on real meetings.
