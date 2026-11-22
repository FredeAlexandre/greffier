"""Tests for the parity bench (the parts without engines): `python3 -m unittest discover -s scripts`."""
from pathlib import Path
import tempfile
import unittest
import wave

import parity_bench


def measure(wer, found, expected, speakers, seconds=60.0):
    return parity_bench.Measure(wer=wer, terms=(found, expected), speakers=speakers, seconds=seconds)


class DecisionTest(unittest.TestCase):
    def test_avfoundation_is_kept_when_it_loses_nothing(self):
        decision = parity_bench.decide(measure(0.080, 58, 67, 0.030), measure(0.081, 58, 67, 0.031))
        self.assertTrue(decision.keep_avfoundation)

    def test_more_than_half_a_point_of_word_errors_bundles_ffmpeg(self):
        decision = parity_bench.decide(measure(0.090, 58, 67, 0.030), measure(0.080, 58, 67, 0.030))
        self.assertFalse(decision.keep_avfoundation)
        self.assertIn("word errors", decision.reasons[0])

    def test_two_lost_key_terms_bundle_ffmpeg(self):
        decision = parity_bench.decide(measure(0.080, 56, 67, 0.030), measure(0.080, 58, 67, 0.030))
        self.assertFalse(decision.keep_avfoundation)
        self.assertIn("key terms", decision.reasons[0])

    def test_one_key_term_difference_is_noise(self):
        decision = parity_bench.decide(measure(0.080, 57, 67, 0.030), measure(0.080, 58, 67, 0.030))
        self.assertTrue(decision.keep_avfoundation)

    def test_more_speaker_errors_bundle_ffmpeg(self):
        decision = parity_bench.decide(measure(0.080, 58, 67, 0.045), measure(0.080, 58, 67, 0.030))
        self.assertFalse(decision.keep_avfoundation)
        self.assertIn("speaker", decision.reasons[0])


class LocationsTest(unittest.TestCase):
    def test_options_win_over_environment_variables(self):
        env = {"GREFFIER_BENCH_EXCERPTS": "/env/excerpts", "GREFFIER_BENCH": "/env/bench"}
        loc = parity_bench.locations(["--excerpts", "/arg/excerpts"], env)
        self.assertEqual((str(loc.excerpts), str(loc.bench)), ("/arg/excerpts", "/env/bench"))

    def test_without_a_location_the_bench_says_what_to_give(self):
        with self.assertRaises(SystemExit) as refusal:
            parity_bench.locations([], {"GREFFIER_BENCH": "/env/bench"})
        self.assertIn("GREFFIER_BENCH_EXCERPTS", str(refusal.exception))


class ReportTest(unittest.TestCase):
    def setUp(self):
        greffier = parity_bench.Measure(wer=0.08, terms=(58, 67), speakers=0.03, seconds=120.0, diarization=30.0)
        vault = parity_bench.Measure(wer=0.073, terms=(59, 67), speakers=None, seconds=126.0)
        self.text = parity_bench.report(
            totals={"Greffier": greffier, "Vault": vault},
            per_excerpt={"A": {"Greffier": greffier, "Vault": vault}},
            decision=parity_bench.Decision(keep_avfoundation=False, reasons=["word errors: 9.0% vs 8.0% with ffmpeg"]),
            notes=["References: A draft."],
        )

    def test_each_system_has_its_four_measures(self):
        self.assertIn("| Greffier | 8.0% | 58/67 | 3.0% | 2.0 min | 0.5 min |", self.text)
        self.assertIn("| Vault | 7.3% | 59/67 | — | 2.1 min | — |", self.text)

    def test_each_excerpt_has_a_row_per_system(self):
        self.assertIn("| A | Greffier | 8.0% | 58/67 | 3.0% | 120 s | 30 s |", self.text)

    def test_the_decision_and_its_reasons_are_recorded(self):
        self.assertIn("ffmpeg bundled", self.text)
        self.assertIn("word errors: 9.0% vs 8.0% with ffmpeg", self.text)
        self.assertIn("References: A draft.", self.text)


def write_wav(path, samples):
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(16000)
        w.writeframes(b"".join(s.to_bytes(2, "little", signed=True) for s in samples))


class ComparisonTest(unittest.TestCase):
    def test_signal_difference_counts_differing_samples_and_their_difference(self):
        d = Path(tempfile.mkdtemp())
        write_wav(d / "a.wav", [0, 100, -100, 3000])
        write_wav(d / "b.wav", [0, 101, -100, 2997, 7])
        diff = parity_bench.signal_difference(d / "a.wav", d / "b.wav")
        self.assertEqual((diff.samples, diff.differing, diff.maximum), (4, 2, 3))
        self.assertEqual(diff.lengths, (4, 5))
        self.assertAlmostEqual(diff.rms, (10 / 4) ** 0.5)

    def test_whisper_words_identical_to_the_vault_account_for_text_and_timestamps(self):
        vault = [{"start": 0.25, "end": 1.07, "text": " Ok,"}, {"start": 1.07, "end": 1.38, "text": " entendu,"},
                 {"start": 1.38, "end": 2.13, "text": " parfait."}]
        greffier = [{"start": 0.25, "end": 1.07, "text": "Ok,"}, {"start": 1.07, "end": 1.40, "text": "entendu,"},
                    {"start": 1.38, "end": 2.13, "text": "parfait."}]
        self.assertEqual(parity_bench.identical_words(greffier, vault), (2, 3))


class CombineTest(unittest.TestCase):
    def test_combining_weights_errors_by_word_count(self):
        a = parity_bench.Measure(wer=0.10, terms=(10, 12), speakers=0.02, seconds=60.0, diarization=30.0, words=1000)
        b = parity_bench.Measure(wer=0.04, terms=(5, 8), speakers=0.04, seconds=40.0, diarization=None, words=500)
        c = parity_bench.combine([a, b])
        self.assertAlmostEqual(c.wer, 120 / 1500)
        self.assertEqual((c.terms, c.words, c.seconds, c.diarization), ((15, 20), 1500, 100.0, None))
        self.assertAlmostEqual(c.speakers, 0.03)


class IntervalTest(unittest.TestCase):
    def test_the_excerpt_interval_comes_from_its_reference_header(self):
        md = "---\ntype: référence\nextrait: A\ndébut: 329.43\nfin: 616.9\nstatut: brouillon\n---\n\n# Extrait A\n"
        self.assertEqual(parity_bench.interval(md), (329.43, 616.9))

    def test_no_interval_gives_nothing(self):
        self.assertIsNone(parity_bench.interval("---\ntype: référence\n---\n"))


class RecordingTest(unittest.TestCase):
    def test_the_original_recording_is_optional(self):
        base = ["--excerpts", "/x", "--bench", "/b"]
        self.assertIsNone(parity_bench.locations(base, {}).recording)
        loc = parity_bench.locations(base, {"GREFFIER_BENCH_RECORDING": "/r/recording.mp4"})
        self.assertEqual(str(loc.recording), "/r/recording.mp4")


class WordParityTest(unittest.TestCase):
    def setUp(self):
        self.vault = [{"start": 0.25, "end": 1.07, "text": " Ok,"}, {"start": 1.07, "end": 1.38, "text": " entendu,"},
                      {"start": 1.38, "end": 2.13, "text": " parfait."}]

    def test_empty_whisper_segments_do_not_count(self):
        greffier = [{"start": 0.23, "end": 0.33, "text": ""}] + [dict(w, text=w["text"].strip()) for w in self.vault]
        self.assertEqual(parity_bench.identical_words(greffier, self.vault), (3, 3))
        self.assertIsNone(parity_bench.first_difference(greffier, self.vault))

    def test_the_first_difference_gives_the_index_and_time_of_the_first_differing_word(self):
        greffier = [dict(w, text=w["text"].strip()) for w in self.vault]
        greffier[1] = {"start": 1.07, "end": 1.40, "text": "entendu."}
        self.assertEqual(parity_bench.first_difference(greffier, self.vault), (1, 1.07))
