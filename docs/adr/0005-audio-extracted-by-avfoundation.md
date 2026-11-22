# The engines' audio is extracted by AVFoundation, not by ffmpeg

Greffier decodes the mixing track with AVFoundation and resamples it itself to 16 kHz (`engines/src/audio.rs`), instead of bundling ffmpeg like the vault. The spec set one condition: lose nothing on the parity bench, a script run by hand that compared Greffier's engines with the vault's pipeline. It was removed from the repository once this decision was taken, so that nothing in Greffier depends on the vault or on Python tooling; it is still in the git history (`git log --all -- scripts/parity_bench.py`, with its engine `engines/examples/bench.rs`). The bench of 2026-11-21 compares AVFoundation and ffmpeg on the same media, with the same engines, on the three 5-minute excerpts (2,479 reference words, still in draft).

| Same media, same engines | Word errors | Correct terms | Wrong speakers |
|---|---:|---:|---:|
| Original recording (Apple AAC), AVFoundation | 6.2% | 61/67 | 2.5% |
| Original recording, ffmpeg (the vault's WAV) | 6.9% | 59/67 | 2.6% |
| m4a re-encoded by ffmpeg, AVFoundation | 6.3% | 62/67 | 2.9% |
| m4a re-encoded by ffmpeg, ffmpeg | 6.8% | 62/67 | 2.4% |
| Vault (whisper-cli, pyannote, assign.ts) | 7.3% | 59/67 | 2.6% |

Combined over both media, AVFoundation gets 6.3% word errors, 123/134 terms and 2.7% speaker errors, against 6.8%, 121/134 and 2.5% with ffmpeg. That is within the tolerances set before the bench (0.5 points of word errors, 1 term, 0.5 points of speaker errors). So we keep AVFoundation. On Apple AAC, the one in Greffier's recordings, the two decodings are almost identical (RMS 0.06 to 0.09 out of 32,768). They diverge more on ffmpeg's AAC (RMS 13 to 22), with no measurable loss. On the vault's WAV, the integrated whisper.cpp returns the same words, to the hundredth of a second, as whisper-cli: excerpts B and C are identical, and A is until word 509, where a rounding difference between the two builds makes the rest diverge.

What we accept: Apple's AAC decoding is not deterministic. Two decodings of the same file differ by one bit on a few dozen samples, and the last 3 milliseconds are sometimes missing. Since Whisper is chaotic, two transcriptions of the same recording can therefore differ by a few words (from 62 to 64 correct terms from one run to the next on the m4a). If references validated by listening overturn these figures, we bring the bench back from the history, rerun it, and bundle ffmpeg as a static binary.
