"""Banc de parité des moteurs (#19), lancé à la main, hors de la suite automatique.

Fait passer les trois extraits de 5 minutes du banc d'essai du vault par les vrais moteurs de
Greffier (`cargo run --release -p greffier-moteurs --example banc`) et les compare au pipeline du
vault : erreurs de mots, termes clés, erreurs de locuteur, durée. Tranche l'extraction audio :
AVFoundation est gardé s'il ne perd rien face à ffmpeg sur le même média avec les mêmes moteurs.

    scripts/banc-parite --extraits <dossier des extraits> --banc <dossier du banc d'essai> \
        [--enregistrement <enregistrement d'origine des extraits>]

(ou GREFFIER_BANC_EXTRAITS, GREFFIER_BANC et GREFFIER_BANC_ENREGISTREMENT). Avec l'enregistrement
d'origine, AVFoundation est aussi comparé à ffmpeg sur lui (le même intervalle que le WAV du banc). Rien n'est copié dans le dépôt : les extraits, les
références et le barème (`scripts/bench.py` du banc) sont lus sur place ; les sorties et le
rapport sont écrits dans `<banc>/greffier/`, le WAV temporaire de pyannote dans le dossier de l'app.
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

DEPOT = Path(__file__).resolve().parent.parent
EXTRAITS = ["A", "B", "C"]
# Le vault séparait les extraits du banc en 2 locuteurs (`bench.py locuteurs`).
LOCUTEURS = 2


@dataclass
class Mesure:
    wer: float
    termes: tuple
    locuteurs: float | None
    # Durée de la transcription, puis de la séparation des locuteurs (None si inconnue).
    secondes: float
    separation: float | None = None
    # Nombre de mots de la référence (pour cumuler les erreurs de mots).
    mots: int = 0


def cumuler(mesures):
    """Plusieurs mesures en une : erreurs de mots pondérées par les mots de référence, termes et
    durées additionnés, erreurs de locuteur moyennées (comme `bench.py score`)."""
    mots = sum(m.mots for m in mesures)
    loc = [m.locuteurs for m in mesures if m.locuteurs is not None]
    sep = None if any(m.separation is None for m in mesures) else sum(m.separation for m in mesures)
    return Mesure(wer=sum(m.wer * m.mots for m in mesures) / mots if mots else 0.0,
                  termes=(sum(m.termes[0] for m in mesures), sum(m.termes[1] for m in mesures)),
                  locuteurs=sum(loc) / len(loc) if loc else None,
                  secondes=sum(m.secondes for m in mesures), separation=sep, mots=mots)


def intervalle(md):
    """(début, fin) de l'extrait dans l'enregistrement d'origine, d'après l'en-tête de sa référence."""
    debut = re.search(r"^début:\s*([\d.]+)\s*$", md, re.M)
    fin = re.search(r"^fin:\s*([\d.]+)\s*$", md, re.M)
    return (float(debut[1]), float(fin[1])) if debut and fin else None


@dataclass
class Decision:
    garder_avfoundation: bool
    raisons: list


# Ce qu'AVFoundation peut perdre face à ffmpeg sur les trois extraits sans que ce soit une perte :
# whisper est chaotique (un écart d'un bit sur un échantillon change parfois un mot), donc un
# écart sous ces seuils est du bruit.
TOLERANCE_WER = 0.005
TOLERANCE_TERMES = 1
TOLERANCE_LOCUTEURS = 0.005


def decider(avfoundation, ffmpeg):
    """AVFoundation est gardé s'il ne perd rien face à ffmpeg (même média, mêmes moteurs)."""
    raisons = []
    if avfoundation.wer - ffmpeg.wer > TOLERANCE_WER:
        raisons.append(f"erreurs de mots : {avfoundation.wer:.1%} contre {ffmpeg.wer:.1%} avec ffmpeg")
    if ffmpeg.termes[0] - avfoundation.termes[0] > TOLERANCE_TERMES:
        raisons.append(f"termes clés : {avfoundation.termes[0]} contre {ffmpeg.termes[0]} avec ffmpeg")
    if (avfoundation.locuteurs is not None and ffmpeg.locuteurs is not None
            and avfoundation.locuteurs - ffmpeg.locuteurs > TOLERANCE_LOCUTEURS):
        raisons.append(f"erreurs de locuteur : {avfoundation.locuteurs:.1%} contre {ffmpeg.locuteurs:.1%} avec ffmpeg")
    return Decision(garder_avfoundation=not raisons, raisons=raisons)


@dataclass
class Emplacements:
    extraits: Path
    banc: Path
    lexique: Path | None = None
    sans_moteurs: bool = False
    enregistrement: Path | None = None


def emplacements(argv, env):
    """Où lire les extraits et le banc : options, sinon variables d'environnement."""
    p = argparse.ArgumentParser(prog="banc-parite", description=__doc__)
    p.add_argument("--extraits", help="dossier des extraits : extrait-X.m4a et « Extrait X.md » (GREFFIER_BANC_EXTRAITS)")
    p.add_argument("--banc", help="dossier du banc d'essai : scripts/bench.py, audio/, sorties/ (GREFFIER_BANC)")
    p.add_argument("--enregistrement", help="enregistrement d'origine des extraits, facultatif (GREFFIER_BANC_ENREGISTREMENT)")
    p.add_argument("--lexique", help="lexique donné à whisper (par défaut celui du dossier de l'app)")
    p.add_argument("--sans-moteurs", action="store_true", help="ne relance pas les moteurs, renote les sorties déjà là")
    a = p.parse_args(argv)
    extraits = a.extraits or env.get("GREFFIER_BANC_EXTRAITS")
    banc = a.banc or env.get("GREFFIER_BANC")
    manque = [n for n, v in (("--extraits ou GREFFIER_BANC_EXTRAITS", extraits), ("--banc ou GREFFIER_BANC", banc)) if not v]
    if manque:
        raise SystemExit("banc-parite : il manque " + " et ".join(manque) + ".")
    enregistrement = a.enregistrement or env.get("GREFFIER_BANC_ENREGISTREMENT")
    return Emplacements(Path(extraits), Path(banc), Path(a.lexique) if a.lexique else None, a.sans_moteurs,
                        Path(enregistrement) if enregistrement else None)


def _pourcent(x):
    return "—" if x is None else f"{x:.1%}"


def _minutes(s):
    return "—" if s is None else f"{s / 60:.1f} min"


def _secondes(s):
    return "—" if s is None else f"{s:.0f} s"


def rapport(totaux, par_extrait, decision, notes):
    """Le rapport comparatif en Markdown : les quatre mesures par système, sur les trois extraits
    puis extrait par extrait, et la décision AVFoundation / ffmpeg."""
    l = ["# Banc de parité des moteurs", ""]
    l += ["## Résultats sur les trois extraits", "",
          "| Système | Erreurs de mots | Termes justes | Locuteurs faux | Transcription | Séparation |",
          "|---|---:|---:|---:|---:|---:|"]
    for nom, m in totaux.items():
        l.append(f"| {nom} | {m.wer:.1%} | {m.termes[0]}/{m.termes[1]} | {_pourcent(m.locuteurs)} "
                 f"| {_minutes(m.secondes)} | {_minutes(m.separation)} |")
    l += ["", "## Par extrait", "",
          "| Extrait | Système | Erreurs de mots | Termes justes | Locuteurs faux | Transcription | Séparation |",
          "|---|---|---:|---:|---:|---:|---:|"]
    for extrait, systemes in par_extrait.items():
        for nom, m in systemes.items():
            l.append(f"| {extrait} | {nom} | {m.wer:.1%} | {m.termes[0]}/{m.termes[1]} | {_pourcent(m.locuteurs)} "
                     f"| {_secondes(m.secondes)} | {_secondes(m.separation)} |")
    l += ["", "## Décision : extraction audio", ""]
    if decision.garder_avfoundation:
        l.append("**AVFoundation gardé** : il ne perd rien face à ffmpeg sur le même média avec les mêmes moteurs "
                 f"(tolérances : {TOLERANCE_WER:.1%} d'erreurs de mots, {TOLERANCE_TERMES} terme clé, "
                 f"{TOLERANCE_LOCUTEURS:.1%} d'erreurs de locuteur).")
    else:
        l.append("**ffmpeg embarqué** (binaire statique) : AVFoundation perd face à ffmpeg sur le même média.")
        l += [f"- {r}" for r in decision.raisons]
    l += ["", "## Notes", ""] + [f"- {n}" for n in notes]
    return "\n".join(l) + "\n"


@dataclass
class Ecart:
    echantillons: int
    differents: int
    maximum: int
    rms: float
    longueurs: tuple


def _echantillons(chemin):
    with wave.open(str(chemin), "rb") as w:
        a = array.array("h")
        a.frombytes(w.readframes(w.getnframes()))
    return a


def ecart_du_signal(wav_a, wav_b):
    """L'écart échantillon par échantillon entre deux WAV PCM 16 bits (sur la longueur commune)."""
    a, b = _echantillons(wav_a), _echantillons(wav_b)
    n = min(len(a), len(b))
    d = [abs(a[i] - b[i]) for i in range(n)]
    rms = math.sqrt(sum(x * x for x in d) / n) if n else 0.0
    return Ecart(n, sum(1 for x in d if x), max(d, default=0), rms, (len(a), len(b)))


def _cles(mots):
    """Les mots de whisper comparables : texte et horodatages, sans les segments vides."""
    return [(m["text"].strip(), round(m["start"], 2), round(m["end"], 2)) for m in mots if m["text"].strip()]


def mots_identiques(mots, reference):
    """(mots identiques, total) : même texte et mêmes horodatages, dans le même ordre."""
    a, b = _cles(mots), _cles(reference)
    blocs = difflib.SequenceMatcher(None, a, b, autojunk=False).get_matching_blocks()
    return sum(x.size for x in blocs), max(len(a), len(b))


def premier_ecart(mots, reference):
    """(rang, instant) du premier mot qui diffère de la référence, ou None si tout est identique."""
    a, b = _cles(mots), _cles(reference)
    for i, (x, y) in enumerate(zip(a, b)):
        if x != y:
            return i, y[1]
    if len(a) != len(b):
        i = min(len(a), len(b))
        return i, (a if len(a) > i else b)[i][1]
    return None


# --- Le banc pour de vrai (moteurs, barème du vault) ---

GREFFIER = "Greffier, m4a décodé par AVFoundation"
FFMPEG = "Moteurs de Greffier, m4a décodé par ffmpeg"
ORIGINE = "Greffier, enregistrement d'origine décodé par AVFoundation"
WAV_DU_VAULT = "Moteurs de Greffier, WAV du vault (ffmpeg, enregistrement d'origine)"
VAULT = "Vault (large-v3 actuel)"
# Nom affiché → sous-dossier de `<banc>/greffier/`.
VARIANTES = {GREFFIER: "m4a-avfoundation", FFMPEG: "m4a-ffmpeg", ORIGINE: "origine-avfoundation", WAV_DU_VAULT: "wav-du-vault"}
# Les comparaisons qui tranchent l'extraction audio : (AVFoundation, ffmpeg) sur le même média,
# avec les mêmes moteurs.
PAIRES = [(GREFFIER, FFMPEG), (ORIGINE, WAV_DU_VAULT)]


class Banc:
    def __init__(self, e):
        self.e = e
        self.sortie = e.banc / "greffier"
        self.variantes = [v for v in VARIANTES if v != ORIGINE or e.enregistrement]

    def entrees(self, x):
        return {
            "m4a": self.e.extraits / f"extrait-{x}.m4a",
            "reference": self.e.extraits / f"Extrait {x}.md",
            "wav_du_vault": self.e.banc / "audio" / f"{x}.wav",
            "vault": self.e.banc / "sorties" / "large-v3-actuel" / f"{x}.json",
            "tours_du_vault": self.e.banc / "sorties" / "_locuteurs" / f"pyannote-{x}.json",
        }

    def verifier(self):
        manque = [str(p) for x in EXTRAITS for p in self.entrees(x).values() if not p.exists()]
        manque += [str(p) for p in [self.e.banc / "scripts" / "bench.py", self.e.lexique, self.e.enregistrement]
                   if p and not p.exists()]
        if manque:
            raise SystemExit("banc-parite : fichiers introuvables :\n  " + "\n  ".join(manque))
        if self.e.enregistrement:
            sans = [x for x in EXTRAITS if not intervalle(self.entrees(x)["reference"].read_text())]
            if sans:
                raise SystemExit(f"banc-parite : pas de « début: » / « fin: » dans la référence des extraits {', '.join(sans)}.")

    def wav(self, nom, x):
        """Le son donné aux moteurs, quand le banc le prépare."""
        return self.sortie / VARIANTES[nom] / f"{x}.wav"

    def second_decodage(self, x):
        return self.sortie / VARIANTES[GREFFIER] / f"{x}-bis.wav"

    def resultat(self, nom, x):
        return self.sortie / VARIANTES[nom] / f"{x}.json"

    def lancer_les_moteurs(self):
        ffmpeg = shutil.which("ffmpeg")
        if not ffmpeg:
            raise SystemExit("banc-parite : ffmpeg est introuvable (il sert de témoin au décodage).")
        cargo = shutil.which("cargo") or str(Path.home() / ".cargo/bin/cargo")
        moteur = [cargo, "run", "--release", "-q", "-p", "greffier-moteurs", "--example", "banc", "--"]
        for nom in self.variantes:
            (self.sortie / VARIANTES[nom]).mkdir(parents=True, exist_ok=True)
        print("Construction du moteur du banc…", flush=True)
        subprocess.run([cargo, "build", "--release", "-q", "-p", "greffier-moteurs", "--example", "banc"], cwd=DEPOT, check=True)
        for x in EXTRAITS:
            e = self.entrees(x)
            # La commande du vault (transcribe.ts), sur le même m4a que Greffier.
            subprocess.run([ffmpeg, "-nostdin", "-loglevel", "error", "-y", "-i", str(e["m4a"]), "-vn", "-ac", "1",
                            "-ar", "16000", "-c:a", "pcm_s16le", str(self.wav(FFMPEG, x))], check=True)
            sons = {GREFFIER: e["m4a"], FFMPEG: self.wav(FFMPEG, x), WAV_DU_VAULT: e["wav_du_vault"]}
            if ORIGINE in self.variantes:
                # Le même intervalle que le WAV du vault (`ffmpeg -ss début -to fin -i enregistrement`),
                # décodé par AVFoundation.
                debut, fin = intervalle(e["reference"].read_text())
                subprocess.run(moteur + ["--decouper", str(self.e.enregistrement), str(debut), str(fin), str(self.wav(ORIGINE, x))],
                               cwd=DEPOT, check=True)
                sons[ORIGINE] = self.wav(ORIGINE, x)
            for nom in self.variantes:
                print(f"{x} · {nom}", flush=True)
                args = moteur + [str(sons[nom]), str(self.resultat(nom, x)), "--locuteurs", str(LOCUTEURS)]
                if self.e.lexique:
                    args += ["--lexique", str(self.e.lexique)]
                if nom == GREFFIER:
                    args += ["--wav", str(self.wav(GREFFIER, x))]
                subprocess.run(args, cwd=DEPOT, check=True)
            # Un second décodage du même m4a, pour voir si AVFoundation rend deux fois le même son.
            subprocess.run(moteur + ["--decouper", str(e["m4a"]), "0", "1e9", str(self.second_decodage(x))], cwd=DEPOT, check=True)

    def bareme(self):
        """Le `scripts/bench.py` du banc, lu sur place, avec les références des extraits donnés."""
        sys.dont_write_bytecode = True
        # bun (assign.ts du vault) n'écrit pas son cache de transpilation.
        os.environ["BUN_RUNTIME_TRANSPILER_CACHE_PATH"] = "0"
        spec = importlib.util.spec_from_file_location("bench_du_vault", self.e.banc / "scripts" / "bench.py")
        bench = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(bench)
        bench.REFS = self.e.extraits
        return bench

    def noter(self):
        """Les mesures de chaque système, extrait par extrait, avec le barème de `bench.py score`."""
        bench = self.bareme()
        refs = bench.references()
        termes = bench.key_terms()
        par_extrait = {}
        for x in EXTRAITS:
            ref_mots, ref_loc = [], []
            for _, qui, texte in refs[x]["lines"]:
                ws = bench.normalize(texte)
                ref_mots += ws
                ref_loc += [qui] * len(ws)
            systemes = {}
            for nom in self.variantes:
                r = json.loads(self.resultat(nom, x).read_text())
                systemes[nom] = (r["words"], [w["locuteur"] for w in r["words"]], r["secondes_transcription"], r["secondes_separation"])
            v = json.loads(self.entrees(x)["vault"].read_text())
            systemes[VAULT] = (v["words"], bench.assign_speakers(v["words"], self.entrees(x)["tours_du_vault"]), v["secondes"], None)
            par_extrait[x] = {}
            for nom, (mots, etiquettes, secondes, separation) in systemes.items():
                hyp_mots, hyp_loc = [], []
                for w, lab in zip(mots, etiquettes):
                    ws = bench.normalize(w["text"])
                    hyp_mots += ws
                    hyp_loc += [lab] * len(ws)
                erreurs = sum(op != "=" for op, _, _ in bench.align(ref_mots, hyp_mots))
                trouves = attendus = 0
                for t in termes:
                    n = bench.count(t, ref_mots)
                    if n:
                        attendus += n
                        trouves += min(n, bench.count(t, hyp_mots))
                loc = bench.speaker_error(ref_mots, ref_loc, hyp_mots, hyp_loc)
                par_extrait[x][nom] = Mesure(erreurs / len(ref_mots), (trouves, attendus), loc, secondes, separation, len(ref_mots))
        totaux = {nom: cumuler([par_extrait[x][nom] for x in EXTRAITS]) for nom in par_extrait[EXTRAITS[0]]}
        statuts = ", ".join(f"{x} {refs[x]['statut']}" for x in EXTRAITS)
        return totaux, par_extrait, statuts

    def decider(self, par_extrait):
        """AVFoundation face à ffmpeg, sur toutes les paires mesurées et les trois extraits à la fois
        (plus de mots, moins de bruit)."""
        paires = [(a, f) for a, f in PAIRES if a in self.variantes]
        avf = cumuler([par_extrait[x][a] for x in EXTRAITS for a, _ in paires])
        ff = cumuler([par_extrait[x][f] for x in EXTRAITS for _, f in paires])
        return decider(avf, ff), paires, avf, ff

    def notes(self, statuts, totaux, paires, avf, ff):
        lexique = self.e.lexique or Path.home() / "Library/Application Support/Greffier/lexique.md"
        n = [f"Lancé le {datetime.datetime.now():%Y-%m-%d %H:%M}, sur {totaux[VAULT].mots} mots de référence.",
             f"Références : {statuts}. Elles partent de la sortie de large-v3 corrigée à la main et l'avantagent "
             "tant qu'elles ne sont pas validées à l'écoute.",
             f"Barème : `{self.e.banc / 'scripts/bench.py'}` du banc (normalisation, alignement, termes clés, "
             f"erreurs de locuteur), locuteurs en {LOCUTEURS} comme `bench.py locuteurs`. Vocabulaire de whisper : "
             f"section Vocabulaire de `{lexique}`.",
             "Durées : transcription et séparation lancées l'une après l'autre (dans l'app elles tournent en "
             "parallèle) ; celles du vault sont celles de `bench.py run` (whisper-cli seul). Elles dépendent de la "
             "charge du Mac pendant le banc.",
             "La décision compare, avec les mêmes moteurs, " + " ; ".join(f"« {a} » à « {f} »" for a, f in paires)
             + f", cumulés : AVFoundation {avf.wer:.1%} d'erreurs de mots, {avf.termes[0]}/{avf.termes[1]} termes, "
               f"{_pourcent(avf.locuteurs)} de locuteurs faux ; ffmpeg {ff.wer:.1%}, {ff.termes[0]}/{ff.termes[1]}, "
               f"{_pourcent(ff.locuteurs)}.",
             "Le m4a des extraits est ré-encodé par ffmpeg depuis l'enregistrement d'origine (OBS, AAC d'Apple) ; "
             "le WAV du banc du vault (`audio/X.wav`) est `ffmpeg -ss début -to fin -i <enregistrement>`."]
        for x in EXTRAITS:
            e = ecart_du_signal(self.wav(GREFFIER, x), self.wav(FFMPEG, x))
            ligne = (f"Extrait {x} : sur le m4a, AVFoundation et ffmpeg diffèrent sur {e.differents}/{e.echantillons} "
                     f"échantillons (écart max {e.maximum}, RMS {e.rms:.2f} sur 32768 ; {e.longueurs[0]} échantillons "
                     f"contre {e.longueurs[1]})")
            if ORIGINE in self.variantes:
                o = ecart_du_signal(self.wav(ORIGINE, x), self.entrees(x)["wav_du_vault"])
                ligne += (f" ; sur l'enregistrement d'origine, sur {o.differents}/{o.echantillons} (écart max {o.maximum}, "
                          f"RMS {o.rms:.2f} ; {o.longueurs[0]} contre {o.longueurs[1]})")
            if self.second_decodage(x).exists():
                d = ecart_du_signal(self.wav(GREFFIER, x), self.second_decodage(x))
                ligne += (f" ; deux décodages AVFoundation du même m4a diffèrent sur {d.differents} échantillons "
                          f"(écart max {d.maximum} ; {d.longueurs[0]} contre {d.longueurs[1]})")
            brut = json.loads(self.resultat(WAV_DU_VAULT, x).read_text())["brut"]
            vault = [w for w in json.loads(self.entrees(x)["vault"].read_text())["words"] if w["text"].strip()]
            same, total = mots_identiques(brut, vault)
            ecart = premier_ecart(brut, vault)
            suite = "aucun écart" if ecart is None else f"premier écart au mot {ecart[0] + 1}, à {ecart[1]:.0f} s"
            n.append(ligne + f". Sur le WAV du vault, whisper de Greffier rend {same}/{total} mots identiques "
                     f"(texte et horodatages) à ceux de whisper-cli dans le vault ({suite}).")
        return n


def main(argv=None, env=None):
    e = emplacements(sys.argv[1:] if argv is None else argv, os.environ if env is None else env)
    banc = Banc(e)
    banc.verifier()
    if not e.sans_moteurs:
        banc.lancer_les_moteurs()
    totaux, par_extrait, statuts = banc.noter()
    decision, paires, avf, ff = banc.decider(par_extrait)
    texte = rapport(totaux, par_extrait, decision, banc.notes(statuts, totaux, paires, avf, ff))
    banc.sortie.mkdir(parents=True, exist_ok=True)
    (banc.sortie / "rapport.md").write_text(texte)
    (banc.sortie / "scores.json").write_text(json.dumps(
        {"totaux": {k: asdict(m) for k, m in totaux.items()},
         "par_extrait": {x: {k: asdict(m) for k, m in s.items()} for x, s in par_extrait.items()},
         "decision": asdict(decision), "avfoundation": asdict(avf), "ffmpeg": asdict(ff)}, ensure_ascii=False, indent=1))
    print(texte)
    print(f"Rapport : {banc.sortie / 'rapport.md'}")


if __name__ == "__main__":
    main()
