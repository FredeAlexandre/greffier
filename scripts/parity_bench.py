"""Engine parity bench (#19), run by hand, outside the automated suite.

Runs the three 5-minute excerpts of the vault's test bench through Greffier's real engines
(`cargo run --release -p greffier-engines --example bench`) and compares them with the vault's
pipeline: word errors, key terms, speaker errors, duration. Settles audio extraction: AVFoundation
is kept if it loses nothing against ffmpeg on the same media with the same engines.

    scripts/parity-bench --excerpts <excerpts folder> --bench <test bench folder> \
        [--recording <original recording of the excerpts>]

(or GREFFIER_BENCH_EXCERPTS, GREFFIER_BENCH and GREFFIER_BENCH_RECORDING). With the original
recording, AVFoundation is also compared with ffmpeg on it (the same interval as the bench's WAV).
Nothing is copied into the repository: the excerpts, the references and the scoring
(the bench's `scripts/bench.py`) are read in place; the outputs and the report are written to
`<bench>/greffier/`, pyannote's temporary WAV to the app folder.
"""
import argparse
import array
from dataclasses import asdict, dataclass
import datetime
import difflib
import importlib.util
import json
import math
import os
import re
from pathlib import Path
import shutil
import subprocess
import sys
import wave

REPO = Path(__file__).resolve().parent.parent
EXCERPTS = ["A", "B", "C"]
# The vault diarized the bench excerpts into 2 speakers (`bench.py locuteurs`).
SPEAKERS = 2


@dataclass
class Measure:
    wer: float
    terms: tuple
    speakers: float | None
    # Duration of the transcription, then of the diarization (None if unknown).
    seconds: float
    diarization: float | None = None
    # Number of words in the reference (to combine word errors).
    words: int = 0


def combine(measures):
    """Several measures as one: word errors weighted by reference words, terms and durations
    summed, speaker errors averaged (like `bench.py score`)."""
    words = sum(m.words for m in measures)
    spk = [m.speakers for m in measures if m.speakers is not None]
    dia = None if any(m.diarization is None for m in measures) else sum(m.diarization for m in measures)
    return Measure(wer=sum(m.wer * m.words for m in measures) / words if words else 0.0,
                   terms=(sum(m.terms[0] for m in measures), sum(m.terms[1] for m in measures)),
                   speakers=sum(spk) / len(spk) if spk else None,
                   seconds=sum(m.seconds for m in measures), diarization=dia, words=words)


def interval(md):
    """(start, end) of the excerpt in the original recording, from its reference's header."""
    start = re.search(r"^début:\s*([\d.]+)\s*$", md, re.M)
    end = re.search(r"^fin:\s*([\d.]+)\s*$", md, re.M)
    return (float(start[1]), float(end[1])) if start and end else None


@dataclass
class Decision:
    keep_avfoundation: bool
    reasons: list


# What AVFoundation may lose against ffmpeg on the three excerpts without it being a loss:
# whisper is chaotic (a one-bit difference on one sample sometimes changes a word), so a
# difference below these thresholds is noise.
TOLERANCE_WER = 0.005
TOLERANCE_TERMS = 1
TOLERANCE_SPEAKERS = 0.005


def decide(avfoundation, ffmpeg):
    """AVFoundation is kept if it loses nothing against ffmpeg (same media, same engines)."""
    reasons = []
    if avfoundation.wer - ffmpeg.wer > TOLERANCE_WER:
        reasons.append(f"word errors: {avfoundation.wer:.1%} vs {ffmpeg.wer:.1%} with ffmpeg")
    if ffmpeg.terms[0] - avfoundation.terms[0] > TOLERANCE_TERMS:
        reasons.append(f"key terms: {avfoundation.terms[0]} vs {ffmpeg.terms[0]} with ffmpeg")
    if (avfoundation.speakers is not None and ffmpeg.speakers is not None
            and avfoundation.speakers - ffmpeg.speakers > TOLERANCE_SPEAKERS):
        reasons.append(f"speaker errors: {avfoundation.speakers:.1%} vs {ffmpeg.speakers:.1%} with ffmpeg")
    return Decision(keep_avfoundation=not reasons, reasons=reasons)


@dataclass
class Locations:
    excerpts: Path
    bench: Path
    lexicon: Path | None = None
    no_engines: bool = False
    recording: Path | None = None


def locations(argv, env):
    """Where to read the excerpts and the bench: options, otherwise environment variables."""
    p = argparse.ArgumentParser(prog="parity-bench", description=__doc__)
    p.add_argument("--excerpts", help="excerpts folder: extrait-X.m4a and \"Extrait X.md\" (GREFFIER_BENCH_EXCERPTS)")
    p.add_argument("--bench", help="test bench folder: scripts/bench.py, audio/, sorties/ (GREFFIER_BENCH)")
    p.add_argument("--recording", help="original recording of the excerpts, optional (GREFFIER_BENCH_RECORDING)")
    p.add_argument("--lexicon", help="lexicon given to whisper (defaults to the one in the app folder)")
    p.add_argument("--no-engines", action="store_true", help="do not rerun the engines, rescore the existing outputs")
    a = p.parse_args(argv)
    excerpts = a.excerpts or env.get("GREFFIER_BENCH_EXCERPTS")
    bench = a.bench or env.get("GREFFIER_BENCH")
    missing = [n for n, v in (("--excerpts or GREFFIER_BENCH_EXCERPTS", excerpts), ("--bench or GREFFIER_BENCH", bench)) if not v]
    if missing:
        raise SystemExit("parity-bench: missing " + " and ".join(missing) + ".")
    recording = a.recording or env.get("GREFFIER_BENCH_RECORDING")
    return Locations(Path(excerpts), Path(bench), Path(a.lexicon) if a.lexicon else None, a.no_engines,
                     Path(recording) if recording else None)


def _percent(x):
    return "—" if x is None else f"{x:.1%}"


def _minutes(s):
    return "—" if s is None else f"{s / 60:.1f} min"


def _seconds(s):
    return "—" if s is None else f"{s:.0f} s"


def report(totals, per_excerpt, decision, notes):
    """The comparison report in Markdown: the four measures per system, over the three excerpts
    then excerpt by excerpt, and the AVFoundation / ffmpeg decision."""
    l = ["# Engine parity bench", ""]
    l += ["## Results over the three excerpts", "",
          "| System | Word errors | Correct terms | Wrong speakers | Transcription | Diarization |",
          "|---|---:|---:|---:|---:|---:|"]
    for name, m in totals.items():
        l.append(f"| {name} | {m.wer:.1%} | {m.terms[0]}/{m.terms[1]} | {_percent(m.speakers)} "
                 f"| {_minutes(m.seconds)} | {_minutes(m.diarization)} |")
    l += ["", "## Per excerpt", "",
          "| Excerpt | System | Word errors | Correct terms | Wrong speakers | Transcription | Diarization |",
          "|---|---|---:|---:|---:|---:|---:|"]
    for excerpt, systems in per_excerpt.items():
        for name, m in systems.items():
            l.append(f"| {excerpt} | {name} | {m.wer:.1%} | {m.terms[0]}/{m.terms[1]} | {_percent(m.speakers)} "
                     f"| {_seconds(m.seconds)} | {_seconds(m.diarization)} |")
    l += ["", "## Decision: audio extraction", ""]
    if decision.keep_avfoundation:
        l.append("**AVFoundation kept**: it loses nothing against ffmpeg on the same media with the same engines "
                 f"(tolerances: {TOLERANCE_WER:.1%} word errors, {TOLERANCE_TERMS} key term, "
                 f"{TOLERANCE_SPEAKERS:.1%} speaker errors).")
    else:
        l.append("**ffmpeg bundled** (static binary): AVFoundation loses against ffmpeg on the same media.")
        l += [f"- {r}" for r in decision.reasons]
    l += ["", "## Notes", ""] + [f"- {n}" for n in notes]
    return "\n".join(l) + "\n"


@dataclass
class Difference:
    samples: int
    differing: int
    maximum: int
    rms: float
    lengths: tuple


def _samples(path):
    with wave.open(str(path), "rb") as w:
        a = array.array("h")
        a.frombytes(w.readframes(w.getnframes()))
    return a


def signal_difference(wav_a, wav_b):
    """The sample-by-sample difference between two 16-bit PCM WAVs (over the common length)."""
    a, b = _samples(wav_a), _samples(wav_b)
    n = min(len(a), len(b))
    d = [abs(a[i] - b[i]) for i in range(n)]
    rms = math.sqrt(sum(x * x for x in d) / n) if n else 0.0
    return Difference(n, sum(1 for x in d if x), max(d, default=0), rms, (len(a), len(b)))


def _keys(words):
    """Comparable whisper words: text and timestamps, without the empty segments."""
    return [(w["text"].strip(), round(w["start"], 2), round(w["end"], 2)) for w in words if w["text"].strip()]


def identical_words(words, reference):
    """(identical words, total): same text and same timestamps, in the same order."""
    a, b = _keys(words), _keys(reference)
    blocks = difflib.SequenceMatcher(None, a, b, autojunk=False).get_matching_blocks()
    return sum(x.size for x in blocks), max(len(a), len(b))


def first_difference(words, reference):
    """(index, time) of the first word that differs from the reference, or None if all are identical."""
    a, b = _keys(words), _keys(reference)
    for i, (x, y) in enumerate(zip(a, b)):
        if x != y:
            return i, y[1]
    if len(a) != len(b):
        i = min(len(a), len(b))
        return i, (a if len(a) > i else b)[i][1]
    return None


# --- The real bench (engines, vault scoring) ---

GREFFIER = "Greffier, m4a decoded by AVFoundation"
FFMPEG = "Greffier engines, m4a decoded by ffmpeg"
ORIGINAL = "Greffier, original recording decoded by AVFoundation"
VAULT_WAV = "Greffier engines, vault WAV (ffmpeg, original recording)"
VAULT = "Vault (current large-v3)"
# Display name → subfolder of `<bench>/greffier/`.
VARIANTS = {GREFFIER: "m4a-avfoundation", FFMPEG: "m4a-ffmpeg", ORIGINAL: "original-avfoundation", VAULT_WAV: "vault-wav"}
# The comparisons that settle audio extraction: (AVFoundation, ffmpeg) on the same media, with
# the same engines.
PAIRS = [(GREFFIER, FFMPEG), (ORIGINAL, VAULT_WAV)]


class Bench:
    def __init__(self, loc):
        self.loc = loc
        self.output = loc.bench / "greffier"
        self.variants = [v for v in VARIANTS if v != ORIGINAL or loc.recording]

    def inputs(self, x):
        return {
            "m4a": self.loc.excerpts / f"extrait-{x}.m4a",
            "reference": self.loc.excerpts / f"Extrait {x}.md",
            "vault_wav": self.loc.bench / "audio" / f"{x}.wav",
            "vault": self.loc.bench / "sorties" / "large-v3-actuel" / f"{x}.json",
            "vault_turns": self.loc.bench / "sorties" / "_locuteurs" / f"pyannote-{x}.json",
        }

    def check(self):
        missing = [str(p) for x in EXCERPTS for p in self.inputs(x).values() if not p.exists()]
        missing += [str(p) for p in [self.loc.bench / "scripts" / "bench.py", self.loc.lexicon, self.loc.recording]
                    if p and not p.exists()]
        if missing:
            raise SystemExit("parity-bench: files not found:\n  " + "\n  ".join(missing))
        if self.loc.recording:
            without = [x for x in EXCERPTS if not interval(self.inputs(x)["reference"].read_text())]
            if without:
                raise SystemExit(f"parity-bench: no \"début:\" / \"fin:\" in the reference of excerpts {', '.join(without)}.")

    def wav(self, name, x):
        """The audio given to the engines, when the bench prepares it."""
        return self.output / VARIANTS[name] / f"{x}.wav"

    def second_decoding(self, x):
        return self.output / VARIANTS[GREFFIER] / f"{x}-bis.wav"

    def result(self, name, x):
        return self.output / VARIANTS[name] / f"{x}.json"

    def run_engines(self):
        ffmpeg = shutil.which("ffmpeg")
        if not ffmpeg:
            raise SystemExit("parity-bench: ffmpeg not found (it is the reference decoder).")
        cargo = shutil.which("cargo") or str(Path.home() / ".cargo/bin/cargo")
        engine = [cargo, "run", "--release", "-q", "-p", "greffier-engines", "--example", "bench", "--"]
        for name in self.variants:
            (self.output / VARIANTS[name]).mkdir(parents=True, exist_ok=True)
        print("Building the bench engine…", flush=True)
        subprocess.run([cargo, "build", "--release", "-q", "-p", "greffier-engines", "--example", "bench"], cwd=REPO, check=True)
        for x in EXCERPTS:
            i = self.inputs(x)
            # The vault's command (transcribe.ts), on the same m4a as Greffier.
            subprocess.run([ffmpeg, "-nostdin", "-loglevel", "error", "-y", "-i", str(i["m4a"]), "-vn", "-ac", "1",
                            "-ar", "16000", "-c:a", "pcm_s16le", str(self.wav(FFMPEG, x))], check=True)
            sources = {GREFFIER: i["m4a"], FFMPEG: self.wav(FFMPEG, x), VAULT_WAV: i["vault_wav"]}
            if ORIGINAL in self.variants:
                # The same interval as the vault's WAV (`ffmpeg -ss start -to end -i recording`),
                # decoded by AVFoundation.
                start, end = interval(i["reference"].read_text())
                subprocess.run(engine + ["--cut", str(self.loc.recording), str(start), str(end), str(self.wav(ORIGINAL, x))],
                               cwd=REPO, check=True)
                sources[ORIGINAL] = self.wav(ORIGINAL, x)
            for name in self.variants:
                print(f"{x} · {name}", flush=True)
                args = engine + [str(sources[name]), str(self.result(name, x)), "--speakers", str(SPEAKERS)]
                if self.loc.lexicon:
                    args += ["--lexicon", str(self.loc.lexicon)]
                if name == GREFFIER:
                    args += ["--wav", str(self.wav(GREFFIER, x))]
                subprocess.run(args, cwd=REPO, check=True)
            # A second decoding of the same m4a, to see whether AVFoundation returns the same audio twice.
            subprocess.run(engine + ["--cut", str(i["m4a"]), "0", "1e9", str(self.second_decoding(x))], cwd=REPO, check=True)

    def scoring(self):
        """The bench's `scripts/bench.py`, read in place, with the references of the given excerpts."""
        sys.dont_write_bytecode = True
        # bun (the vault's assign.ts) does not write its transpilation cache.
        os.environ["BUN_RUNTIME_TRANSPILER_CACHE_PATH"] = "0"
        spec = importlib.util.spec_from_file_location("vault_bench", self.loc.bench / "scripts" / "bench.py")
        bench = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(bench)
        bench.REFS = self.loc.excerpts
        return bench

    def score(self):
        """Each system's measures, excerpt by excerpt, with the scoring of `bench.py score`."""
        bench = self.scoring()
        refs = bench.references()
        terms = bench.key_terms()
        per_excerpt = {}
        for x in EXCERPTS:
            ref_words, ref_spk = [], []
            for _, who, text in refs[x]["lines"]:
                ws = bench.normalize(text)
                ref_words += ws
                ref_spk += [who] * len(ws)
            systems = {}
            for name in self.variants:
                r = json.loads(self.result(name, x).read_text())
                systems[name] = (r["words"], [w["speaker"] for w in r["words"]], r["transcription_seconds"], r["diarization_seconds"])
            v = json.loads(self.inputs(x)["vault"].read_text())
            systems[VAULT] = (v["words"], bench.assign_speakers(v["words"], self.inputs(x)["vault_turns"]), v["secondes"], None)
            per_excerpt[x] = {}
            for name, (words, labels, seconds, diarization) in systems.items():
                hyp_words, hyp_spk = [], []
                for w, lab in zip(words, labels):
                    ws = bench.normalize(w["text"])
                    hyp_words += ws
                    hyp_spk += [lab] * len(ws)
                errors = sum(op != "=" for op, _, _ in bench.align(ref_words, hyp_words))
                found = expected = 0
                for t in terms:
                    n = bench.count(t, ref_words)
                    if n:
                        expected += n
                        found += min(n, bench.count(t, hyp_words))
                spk = bench.speaker_error(ref_words, ref_spk, hyp_words, hyp_spk)
                per_excerpt[x][name] = Measure(errors / len(ref_words), (found, expected), spk, seconds, diarization, len(ref_words))
        totals = {name: combine([per_excerpt[x][name] for x in EXCERPTS]) for name in per_excerpt[EXCERPTS[0]]}
        statuses = ", ".join(f"{x} {refs[x]['statut']}" for x in EXCERPTS)
        return totals, per_excerpt, statuses

    def decide(self, per_excerpt):
        """AVFoundation against ffmpeg, over all measured pairs and the three excerpts at once
        (more words, less noise)."""
        pairs = [(a, f) for a, f in PAIRS if a in self.variants]
        avf = combine([per_excerpt[x][a] for x in EXCERPTS for a, _ in pairs])
        ff = combine([per_excerpt[x][f] for x in EXCERPTS for _, f in pairs])
        return decide(avf, ff), pairs, avf, ff

    def notes(self, statuses, totals, pairs, avf, ff):
        lexicon = self.loc.lexicon or Path.home() / "Library/Application Support/Greffier/lexique.md"
        n = [f"Run on {datetime.datetime.now():%Y-%m-%d %H:%M}, over {totals[VAULT].words} reference words.",
             f"References: {statuses}. They start from large-v3's output corrected by hand and favor it "
             "until they are validated by listening.",
             f"Scoring: the bench's `{self.loc.bench / 'scripts/bench.py'}` (normalization, alignment, key terms, "
             f"speaker errors), {SPEAKERS} speakers like `bench.py locuteurs`. Whisper vocabulary: "
             f"Vocabulaire section of `{lexicon}`.",
             "Durations: transcription and diarization run one after the other (in the app they run in "
             "parallel); the vault's are those of `bench.py run` (whisper-cli alone). They depend on the "
             "Mac's load during the bench.",
             "The decision compares, with the same engines, " + "; ".join(f"\"{a}\" with \"{f}\"" for a, f in pairs)
             + f", combined: AVFoundation {avf.wer:.1%} word errors, {avf.terms[0]}/{avf.terms[1]} terms, "
               f"{_percent(avf.speakers)} wrong speakers; ffmpeg {ff.wer:.1%}, {ff.terms[0]}/{ff.terms[1]}, "
               f"{_percent(ff.speakers)}.",
             "The excerpts' m4a is re-encoded by ffmpeg from the original recording (OBS, Apple AAC); "
             "the vault bench's WAV (`audio/X.wav`) is `ffmpeg -ss start -to end -i <recording>`."]
        for x in EXCERPTS:
            d = signal_difference(self.wav(GREFFIER, x), self.wav(FFMPEG, x))
            line = (f"Excerpt {x}: on the m4a, AVFoundation and ffmpeg differ on {d.differing}/{d.samples} "
                    f"samples (max difference {d.maximum}, RMS {d.rms:.2f} out of 32768; {d.lengths[0]} samples "
                    f"vs {d.lengths[1]})")
            if ORIGINAL in self.variants:
                o = signal_difference(self.wav(ORIGINAL, x), self.inputs(x)["vault_wav"])
                line += (f"; on the original recording, on {o.differing}/{o.samples} (max difference {o.maximum}, "
                         f"RMS {o.rms:.2f}; {o.lengths[0]} vs {o.lengths[1]})")
            if self.second_decoding(x).exists():
                s = signal_difference(self.wav(GREFFIER, x), self.second_decoding(x))
                line += (f"; two AVFoundation decodings of the same m4a differ on {s.differing} samples "
                         f"(max difference {s.maximum}; {s.lengths[0]} vs {s.lengths[1]})")
            raw = json.loads(self.result(VAULT_WAV, x).read_text())["raw"]
            vault = [w for w in json.loads(self.inputs(x)["vault"].read_text())["words"] if w["text"].strip()]
            same, total = identical_words(raw, vault)
            first = first_difference(raw, vault)
            rest = "no difference" if first is None else f"first difference at word {first[0] + 1}, at {first[1]:.0f} s"
            n.append(line + f". On the vault WAV, Greffier's whisper returns {same}/{total} words identical "
                     f"(text and timestamps) to whisper-cli's in the vault ({rest}).")
        return n


def main(argv=None, env=None):
    loc = locations(sys.argv[1:] if argv is None else argv, os.environ if env is None else env)
    bench = Bench(loc)
    bench.check()
    if not loc.no_engines:
        bench.run_engines()
    totals, per_excerpt, statuses = bench.score()
    decision, pairs, avf, ff = bench.decide(per_excerpt)
    text = report(totals, per_excerpt, decision, bench.notes(statuses, totals, pairs, avf, ff))
    bench.output.mkdir(parents=True, exist_ok=True)
    (bench.output / "report.md").write_text(text)
    (bench.output / "scores.json").write_text(json.dumps(
        {"totals": {k: asdict(m) for k, m in totals.items()},
         "per_excerpt": {x: {k: asdict(m) for k, m in s.items()} for x, s in per_excerpt.items()},
         "decision": asdict(decision), "avfoundation": asdict(avf), "ffmpeg": asdict(ff)}, ensure_ascii=False, indent=1))
    print(text)
    print(f"Report: {bench.output / 'report.md'}")


if __name__ == "__main__":
    main()
