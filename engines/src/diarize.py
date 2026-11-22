"""
Qui parle quand : la séparation des locuteurs d'un WAV 16 kHz mono 16 bits, par pyannote.

    python -I -c "<ce script>" AUDIO.wav SORTIE.json MODELE [--locuteurs N]

Lancé par Greffier (moteurs/src/pyannote.rs) dans son Python privé, avec l'environnement de
`greffier_outils::mac::environnement_python` (hors ligne, rien d'écrit hors du dossier de l'app),
pendant que whisper.cpp transcrit. MODELE est le dossier pyannote-community-1 installé au premier
lancement ; il est lu hors ligne, sans jeton.

Sortie exclusive (un seul locuteur à la fois : celle qui s'aligne sur une transcription), sur MPS
avec repli sur le CPU en cas d'erreur. Écrit
{"segments": [{"start", "end", "speaker"}], "speakers": n, "device": "mps", "seconds": s}
où `speaker` est un entier, 0 pour le premier locuteur rendu par pyannote. Sur la sortie
d'erreur, des lignes « PROGRES 0.42 » donnent l'avancement.
"""

import json
import os
import sys
import time
import warnings
import wave

os.environ.setdefault("PYTORCH_ENABLE_MPS_FALLBACK", "1")
os.environ.setdefault("HF_HUB_OFFLINE", "1")
warnings.filterwarnings("ignore")  # torchcodec / torchaudio : on ne décode aucun fichier avec eux

import numpy as np  # noqa: E402
import torch  # noqa: E402
from pyannote.audio import Pipeline  # noqa: E402

FREQUENCE = 16000
# Part de l'avancement de chaque étape de pyannote (les empreintes de voix sont les plus longues).
ETAPES = {"segmentation": (0.0, 0.2), "embeddings": (0.2, 0.95)}


def progres(p: float) -> None:
    print(f"PROGRES {p:.3f}", file=sys.stderr, flush=True)


def crochet(etape, _artefact, file=None, total=None, completed=None):
    debut, fin = ETAPES.get(etape, (None, None))
    if debut is None:
        return
    if total and completed is not None:
        progres(debut + (fin - debut) * min(completed / total, 1.0))
    elif completed is None and total is None:
        progres(fin)


def lire_wav(chemin: str) -> torch.Tensor:
    with wave.open(chemin, "rb") as w:
        if w.getframerate() != FREQUENCE or w.getnchannels() != 1 or w.getsampwidth() != 2:
            raise SystemExit(f"{chemin} : il faut un WAV 16 kHz mono 16 bits")
        trames = w.readframes(w.getnframes())
    audio = np.frombuffer(trames, dtype="<i2").astype(np.float32) / 32768.0
    return torch.from_numpy(audio).unsqueeze(0)


def exclusive(sortie):
    """pyannote 4 rend un DiarizeOutput ; on prend sa version exclusive."""
    for nom in ("exclusive_speaker_diarization", "speaker_diarization"):
        a = getattr(sortie, nom, None)
        if a is not None:
            return a
    return sortie


def separer(modele: str, audio: torch.Tensor, locuteurs, appareil: str):
    pipeline = Pipeline.from_pretrained(modele)
    if pipeline is None:
        raise SystemExit(f"modèle pyannote illisible : {modele}")
    pipeline.to(torch.device(appareil))
    options = {"num_speakers": locuteurs} if locuteurs else {}
    return exclusive(pipeline({"waveform": audio, "sample_rate": FREQUENCE}, hook=crochet, **options))


def main() -> int:
    args = sys.argv[1:]
    locuteurs = None
    if "--locuteurs" in args:
        i = args.index("--locuteurs")
        locuteurs = int(args[i + 1]) or None
        del args[i:i + 2]
    if len(args) != 3:
        print("Usage : AUDIO.wav SORTIE.json MODELE [--locuteurs N]", file=sys.stderr)
        return 2
    wav, sortie, modele = args

    t0 = time.time()
    audio = lire_wav(wav)
    appareil = "mps" if torch.backends.mps.is_available() else "cpu"
    try:
        annotation = separer(modele, audio, locuteurs, appareil)
    except Exception as erreur:  # noqa: BLE001
        if appareil == "cpu":
            raise
        print(f"pyannote sur {appareil} : {erreur} ; nouvel essai sur le CPU", file=sys.stderr, flush=True)
        appareil = "cpu"
        annotation = separer(modele, audio, locuteurs, appareil)

    numeros: dict[str, int] = {}
    segments = []
    for tour, _, etiquette in annotation.itertracks(yield_label=True):
        n = numeros.setdefault(etiquette, len(numeros))
        segments.append({"start": round(tour.start, 3), "end": round(tour.end, 3), "speaker": n})
    segments.sort(key=lambda s: s["start"])

    with open(sortie, "w", encoding="utf-8") as f:
        json.dump({"segments": segments, "speakers": len(numeros), "device": appareil, "seconds": round(time.time() - t0, 1)}, f)
    progres(1.0)
    return 0


if __name__ == "__main__":
    sys.exit(main())
