"""
Who speaks when: diarization of a 16 kHz mono 16-bit WAV, by pyannote.

    python -I -c "<this script>" AUDIO.wav OUTPUT.json MODEL [--speakers N]

Run by Greffier (engines/src/pyannote.rs) in its private Python, with the environment from
`greffier_tools::mac::python_env` (offline, nothing written outside the app folder), while
whisper.cpp transcribes. MODEL is the pyannote-community-1 folder installed on first launch; it is
read offline, without a token.

Exclusive output (one speaker at a time: the one that lines up with a transcription), on MPS with
a fallback to the CPU on error. Writes
{"segments": [{"start", "end", "speaker"}], "speakers": n, "device": "mps", "seconds": s}
where `speaker` is an integer, 0 for the first speaker returned by pyannote. On stderr,
"PROGRESS 0.42" lines give the progress.
"""

import json
import os
import sys
import time
import warnings
import wave

os.environ.setdefault("PYTORCH_ENABLE_MPS_FALLBACK", "1")
os.environ.setdefault("HF_HUB_OFFLINE", "1")
warnings.filterwarnings("ignore")  # torchcodec / torchaudio: no file is decoded with them

import numpy as np  # noqa: E402
import torch  # noqa: E402
from pyannote.audio import Pipeline  # noqa: E402

SAMPLE_RATE = 16000
# Share of the progress for each pyannote step (the voice embeddings take the longest).
STEPS = {"segmentation": (0.0, 0.2), "embeddings": (0.2, 0.95)}


def progress(p: float) -> None:
    print(f"PROGRESS {p:.3f}", file=sys.stderr, flush=True)


def hook(step, _artifact, file=None, total=None, completed=None):
    start, end = STEPS.get(step, (None, None))
    if start is None:
        return
    if total and completed is not None:
        progress(start + (end - start) * min(completed / total, 1.0))
    elif completed is None and total is None:
        progress(end)


def read_wav(path: str) -> torch.Tensor:
    with wave.open(path, "rb") as w:
        if w.getframerate() != SAMPLE_RATE or w.getnchannels() != 1 or w.getsampwidth() != 2:
            raise SystemExit(f"{path} : il faut un WAV 16 kHz mono 16 bits")
        frames = w.readframes(w.getnframes())
    audio = np.frombuffer(frames, dtype="<i2").astype(np.float32) / 32768.0
    return torch.from_numpy(audio).unsqueeze(0)


def exclusive(output):
    """pyannote 4 returns a DiarizeOutput; take its exclusive version."""
    for name in ("exclusive_speaker_diarization", "speaker_diarization"):
        a = getattr(output, name, None)
        if a is not None:
            return a
    return output


def diarize(model: str, audio: torch.Tensor, speakers, device: str):
    pipeline = Pipeline.from_pretrained(model)
    if pipeline is None:
        raise SystemExit(f"modèle pyannote illisible : {model}")
    pipeline.to(torch.device(device))
    options = {"num_speakers": speakers} if speakers else {}
    return exclusive(pipeline({"waveform": audio, "sample_rate": SAMPLE_RATE}, hook=hook, **options))


def main() -> int:
    args = sys.argv[1:]
    speakers = None
    if "--speakers" in args:
        i = args.index("--speakers")
        speakers = int(args[i + 1]) or None
        del args[i:i + 2]
    if len(args) != 3:
        print("Usage: AUDIO.wav OUTPUT.json MODEL [--speakers N]", file=sys.stderr)
        return 2
    wav, output, model = args

    t0 = time.time()
    audio = read_wav(wav)
    device = "mps" if torch.backends.mps.is_available() else "cpu"
    try:
        annotation = diarize(model, audio, speakers, device)
    except Exception as error:  # noqa: BLE001
        if device == "cpu":
            raise
        print(f"pyannote sur {device} : {error} ; nouvel essai sur le CPU", file=sys.stderr, flush=True)
        device = "cpu"
        annotation = diarize(model, audio, speakers, device)

    numbers: dict[str, int] = {}
    segments = []
    for turn, _, label in annotation.itertracks(yield_label=True):
        n = numbers.setdefault(label, len(numbers))
        segments.append({"start": round(turn.start, 3), "end": round(turn.end, 3), "speaker": n})
    segments.sort(key=lambda s: s["start"])

    with open(output, "w", encoding="utf-8") as f:
        json.dump({"segments": segments, "speakers": len(numbers), "device": device, "seconds": round(time.time() - t0, 1)}, f)
    progress(1.0)
    return 0


if __name__ == "__main__":
    sys.exit(main())
