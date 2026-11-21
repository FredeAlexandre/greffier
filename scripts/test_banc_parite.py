"""Tests du banc de parité (les parties sans moteur) : `python3 -m unittest discover -s scripts`."""
from pathlib import Path
import tempfile
import unittest
import wave

import banc_parite


def mesure(wer, trouves, attendus, locuteurs, secondes=60.0):
    return banc_parite.Mesure(wer=wer, termes=(trouves, attendus), locuteurs=locuteurs, secondes=secondes)


class DecisionTest(unittest.TestCase):
    def test_avfoundation_est_garde_quand_il_ne_perd_rien(self):
        decision = banc_parite.decider(mesure(0.080, 58, 67, 0.030), mesure(0.081, 58, 67, 0.031))
        self.assertTrue(decision.garder_avfoundation)

    def test_plus_d_un_demi_point_d_erreurs_de_mots_fait_embarquer_ffmpeg(self):
        decision = banc_parite.decider(mesure(0.090, 58, 67, 0.030), mesure(0.080, 58, 67, 0.030))
        self.assertFalse(decision.garder_avfoundation)
        self.assertIn("erreurs de mots", decision.raisons[0])

    def test_deux_termes_cles_perdus_font_embarquer_ffmpeg(self):
        decision = banc_parite.decider(mesure(0.080, 56, 67, 0.030), mesure(0.080, 58, 67, 0.030))
        self.assertFalse(decision.garder_avfoundation)
        self.assertIn("termes clés", decision.raisons[0])

    def test_un_terme_cle_d_ecart_reste_dans_le_bruit(self):
        decision = banc_parite.decider(mesure(0.080, 57, 67, 0.030), mesure(0.080, 58, 67, 0.030))
        self.assertTrue(decision.garder_avfoundation)

    def test_plus_d_erreurs_de_locuteur_font_embarquer_ffmpeg(self):
        decision = banc_parite.decider(mesure(0.080, 58, 67, 0.045), mesure(0.080, 58, 67, 0.030))
        self.assertFalse(decision.garder_avfoundation)
        self.assertIn("locuteur", decision.raisons[0])


class EmplacementsTest(unittest.TestCase):
    def test_les_options_l_emportent_sur_les_variables_d_environnement(self):
        env = {"GREFFIER_BANC_EXTRAITS": "/env/extraits", "GREFFIER_BANC": "/env/banc"}
        e = banc_parite.emplacements(["--extraits", "/arg/extraits"], env)
        self.assertEqual((str(e.extraits), str(e.banc)), ("/arg/extraits", "/env/banc"))

    def test_sans_emplacement_le_banc_dit_quoi_donner(self):
        with self.assertRaises(SystemExit) as refus:
            banc_parite.emplacements([], {"GREFFIER_BANC": "/env/banc"})
        self.assertIn("GREFFIER_BANC_EXTRAITS", str(refus.exception))


class RapportTest(unittest.TestCase):
    def setUp(self):
        greffier = banc_parite.Mesure(wer=0.08, termes=(58, 67), locuteurs=0.03, secondes=120.0, separation=30.0)
        vault = banc_parite.Mesure(wer=0.073, termes=(59, 67), locuteurs=None, secondes=126.0)
        self.texte = banc_parite.rapport(
            totaux={"Greffier": greffier, "Vault": vault},
            par_extrait={"A": {"Greffier": greffier, "Vault": vault}},
            decision=banc_parite.Decision(garder_avfoundation=False, raisons=["erreurs de mots : 9.0% contre 8.0% avec ffmpeg"]),
            notes=["Références : A brouillon."],
        )

    def test_chaque_systeme_a_ses_quatre_mesures(self):
        self.assertIn("| Greffier | 8.0% | 58/67 | 3.0% | 2.0 min | 0.5 min |", self.texte)
        self.assertIn("| Vault | 7.3% | 59/67 | — | 2.1 min | — |", self.texte)

    def test_chaque_extrait_a_sa_ligne_par_systeme(self):
        self.assertIn("| A | Greffier | 8.0% | 58/67 | 3.0% | 120 s | 30 s |", self.texte)

    def test_la_decision_et_ses_raisons_sont_consignees(self):
        self.assertIn("ffmpeg embarqué", self.texte)
        self.assertIn("erreurs de mots : 9.0% contre 8.0% avec ffmpeg", self.texte)
        self.assertIn("Références : A brouillon.", self.texte)


def ecrire_wav(chemin, echantillons):
    with wave.open(str(chemin), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(16000)
        w.writeframes(b"".join(e.to_bytes(2, "little", signed=True) for e in echantillons))


class ComparaisonTest(unittest.TestCase):
    def test_l_ecart_du_signal_compte_les_echantillons_differents_et_leur_ecart(self):
        d = Path(tempfile.mkdtemp())
        ecrire_wav(d / "a.wav", [0, 100, -100, 3000])
        ecrire_wav(d / "b.wav", [0, 101, -100, 2997, 7])
        ecart = banc_parite.ecart_du_signal(d / "a.wav", d / "b.wav")
        self.assertEqual((ecart.echantillons, ecart.differents, ecart.maximum), (4, 2, 3))
        self.assertEqual(ecart.longueurs, (4, 5))
        self.assertAlmostEqual(ecart.rms, (10 / 4) ** 0.5)

    def test_les_mots_de_whisper_identiques_au_vault_tiennent_compte_du_texte_et_des_horodatages(self):
        vault = [{"start": 0.25, "end": 1.07, "text": " Ok,"}, {"start": 1.07, "end": 1.38, "text": " entendu,"},
                 {"start": 1.38, "end": 2.13, "text": " parfait."}]
        greffier = [{"start": 0.25, "end": 1.07, "text": "Ok,"}, {"start": 1.07, "end": 1.40, "text": "entendu,"},
                    {"start": 1.38, "end": 2.13, "text": "parfait."}]
        self.assertEqual(banc_parite.mots_identiques(greffier, vault), (2, 3))


class CumulTest(unittest.TestCase):
    def test_le_cumul_pondere_les_erreurs_par_le_nombre_de_mots(self):
        a = banc_parite.Mesure(wer=0.10, termes=(10, 12), locuteurs=0.02, secondes=60.0, separation=30.0, mots=1000)
        b = banc_parite.Mesure(wer=0.04, termes=(5, 8), locuteurs=0.04, secondes=40.0, separation=None, mots=500)
        c = banc_parite.cumuler([a, b])
        self.assertAlmostEqual(c.wer, 120 / 1500)
        self.assertEqual((c.termes, c.mots, c.secondes, c.separation), ((15, 20), 1500, 100.0, None))
        self.assertAlmostEqual(c.locuteurs, 0.03)


class IntervalleTest(unittest.TestCase):
    def test_l_intervalle_de_l_extrait_vient_de_l_en_tete_de_sa_reference(self):
        md = "---\ntype: référence\nextrait: A\ndébut: 329.43\nfin: 616.9\nstatut: brouillon\n---\n\n# Extrait A\n"
        self.assertEqual(banc_parite.intervalle(md), (329.43, 616.9))

    def test_sans_intervalle_rien(self):
        self.assertIsNone(banc_parite.intervalle("---\ntype: référence\n---\n"))


class EnregistrementTest(unittest.TestCase):
    def test_l_enregistrement_d_origine_est_facultatif(self):
        base = ["--extraits", "/x", "--banc", "/b"]
        self.assertIsNone(banc_parite.emplacements(base, {}).enregistrement)
        e = banc_parite.emplacements(base, {"GREFFIER_BANC_ENREGISTREMENT": "/r/enregistrement.mp4"})
        self.assertEqual(str(e.enregistrement), "/r/enregistrement.mp4")


class ParitéDesMotsTest(unittest.TestCase):
    def setUp(self):
        self.vault = [{"start": 0.25, "end": 1.07, "text": " Ok,"}, {"start": 1.07, "end": 1.38, "text": " entendu,"},
                      {"start": 1.38, "end": 2.13, "text": " parfait."}]

    def test_les_segments_vides_de_whisper_ne_comptent_pas(self):
        greffier = [{"start": 0.23, "end": 0.33, "text": ""}] + [dict(m, text=m["text"].strip()) for m in self.vault]
        self.assertEqual(banc_parite.mots_identiques(greffier, self.vault), (3, 3))
        self.assertIsNone(banc_parite.premier_ecart(greffier, self.vault))

    def test_le_premier_ecart_donne_le_rang_et_l_instant_du_premier_mot_different(self):
        greffier = [dict(m, text=m["text"].strip()) for m in self.vault]
        greffier[1] = {"start": 1.07, "end": 1.40, "text": "entendu."}
        self.assertEqual(banc_parite.premier_ecart(greffier, self.vault), (1, 1.07))
