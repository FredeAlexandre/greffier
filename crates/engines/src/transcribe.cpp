// A whisper.cpp transcription with the settings of the vault's command:
//
//     whisper-cli -m ggml-large-v3.bin -f audio.wav -l fr -t 6 \
//       [--prompt "<vocabulary>" --carry-initial-prompt] -ml 1 -sow -ojf \
//       --vad -vm ggml-silero-v5.1.2.bin -pp -np
//
// The parameters are set exactly as examples/cli/cli.cpp sets them (whisper-cli defaults for
// everything else: beam 5, best-of 5, temperature 0 with a 0.2 fallback step…). Each segment (one
// word, thanks to -ml 1 -sow) is returned with its confidence: the minimum `p` of its tokens,
// excluding special tokens ("[_BEG_]", "[_TT_…]"), as the vault read it from the full JSON (split
// UTF-8 tokens grouped, `p` of the first one).

#include "whisper.h"

#include <atomic>
#include <cfloat>
#include <cstring>
#include <string>

extern "C" {

struct greffier_settings {
    const char * model;
    const char * vad;
    const char * language;
    int threads;
    // Empty: no prompt.
    const char * vocabulary;
};

typedef void (*greffier_word)(void * user, const char * text, int64_t t0_ms, int64_t t1_ms, float p);
typedef void (*greffier_progress)(void * user, int progress);
typedef bool (*greffier_abort)(void * user);

static std::string last_error;

static void log_callback(enum ggml_log_level level, const char * text, void *) {
    if (level == GGML_LOG_LEVEL_ERROR && text) {
        last_error = text;
    }
}

// utf8_trailing_bytes_needed from examples/common-whisper.cpp.
static int utf8_missing_bytes(const std::string & s) {
    const int n = (int) s.size();
    int i = n - 1;
    while (i >= 0 && ((unsigned char) s[i] & 0xC0) == 0x80) {
        --i;
    }
    if (i < 0) {
        return 0;
    }
    const unsigned char c = (unsigned char) s[i];
    int expected;
    if ((c & 0x80) == 0x00) {
        expected = 1;
    } else if ((c & 0xE0) == 0xC0) {
        expected = 2;
    } else if ((c & 0xF0) == 0xE0) {
        expected = 3;
    } else if ((c & 0xF8) == 0xF0) {
        expected = 4;
    } else {
        return 0;
    }
    const int present = n - i;
    return present >= expected ? 0 : (expected - present);
}

struct callbacks {
    void * user;
    greffier_progress progress;
    greffier_abort should_abort;
};

// 0: done; 1: model unreadable; 2: whisper failed; 3: aborted.
int greffier_transcribe(const greffier_settings * r, const float * pcm, int n, void * user, greffier_word word, greffier_progress progress, greffier_abort should_abort,
                        char * error, int error_size) {
    whisper_log_set(log_callback, nullptr);
    last_error.clear();
    auto return_error = [&](const char * fallback) {
        const std::string & m = last_error.empty() ? std::string(fallback) : last_error;
        if (error_size > 0) {
            std::strncpy(error, m.c_str(), error_size - 1);
            error[error_size - 1] = 0;
        }
    };

    whisper_context_params cparams = whisper_context_default_params();
    cparams.use_gpu = true;
    cparams.gpu_device = 0;
    cparams.flash_attn = true;
    whisper_context * ctx = whisper_init_from_file_with_params(r->model, cparams);
    if (!ctx) {
        return_error("impossible de charger le modèle");
        return 1;
    }

    const bool with_vocabulary = r->vocabulary && r->vocabulary[0];

    whisper_full_params w = whisper_full_default_params(WHISPER_SAMPLING_GREEDY);
    const int beam_size = whisper_full_default_params(WHISPER_SAMPLING_BEAM_SEARCH).beam_search.beam_size;
    const int best_of = whisper_full_default_params(WHISPER_SAMPLING_GREEDY).greedy.best_of;
    w.strategy = beam_size > 1 ? WHISPER_SAMPLING_BEAM_SEARCH : WHISPER_SAMPLING_GREEDY;
    w.print_realtime = false;
    w.print_progress = false;  // -pp: progress goes through the callback
    w.print_timestamps = true;
    w.print_special = false;
    w.translate = false;
    w.language = r->language;
    w.detect_language = false;
    w.n_threads = r->threads;
    w.offset_ms = 0;
    w.duration_ms = 0;
    w.token_timestamps = true;  // -ojf, -ml 1
    w.thold_pt = 0.01f;
    w.max_len = 1;
    w.split_on_word = true;
    w.audio_ctx = 0;
    w.debug_mode = false;
    w.tdrz_enable = false;
    w.suppress_regex = nullptr;
    w.initial_prompt = with_vocabulary ? r->vocabulary : "";
    w.carry_initial_prompt = with_vocabulary;
    w.greedy.best_of = best_of;
    w.beam_search.beam_size = beam_size;
    w.temperature_inc = 0.2f;
    w.temperature = 0.0f;
    w.entropy_thold = 2.40f;
    w.logprob_thold = -1.00f;
    w.no_speech_thold = 0.6f;
    w.no_timestamps = false;
    w.suppress_nst = false;
    w.vad = true;
    w.vad_model_path = r->vad;
    w.vad_params.threshold = 0.5f;
    w.vad_params.min_speech_duration_ms = 250;
    w.vad_params.min_silence_duration_ms = 100;
    w.vad_params.max_speech_duration_s = FLT_MAX;
    w.vad_params.speech_pad_ms = 30;
    w.vad_params.samples_overlap = 0.1f;

    callbacks rp{user, progress, should_abort};
    w.progress_callback = [](whisper_context *, whisper_state *, int p, void * u) {
        auto * rp = (callbacks *) u;
        rp->progress(rp->user, p);
    };
    w.progress_callback_user_data = &rp;
    w.abort_callback = [](void * u) {
        auto * rp = (callbacks *) u;
        return rp->should_abort(rp->user);
    };
    w.abort_callback_user_data = &rp;
    w.encoder_begin_callback = [](whisper_context *, whisper_state *, void * u) {
        auto * rp = (callbacks *) u;
        return !rp->should_abort(rp->user);
    };
    w.encoder_begin_callback_user_data = &rp;

    const int res = whisper_full(ctx, w, pcm, n);
    if (should_abort(user)) {
        whisper_free(ctx);
        return 3;
    }
    if (res != 0) {
        return_error("whisper a échoué");
        whisper_free(ctx);
        return 2;
    }

    const int n_segments = whisper_full_n_segments(ctx);
    for (int i = 0; i < n_segments; ++i) {
        const char * text = whisper_full_get_segment_text(ctx, i);
        const int64_t t0 = whisper_full_get_segment_t0(ctx, i) * 10;
        const int64_t t1 = whisper_full_get_segment_t1(ctx, i) * 10;
        float p = 1.0f;
        bool seen = false;
        const int nt = whisper_full_n_tokens(ctx, i);
        for (int j = 0; j < nt;) {
            whisper_token_data d = whisper_full_get_token_data(ctx, i, j);
            std::string t = whisper_token_to_str(ctx, d.id);
            ++j;
            while (j < nt && utf8_missing_bytes(t) > 0) {
                t += whisper_token_to_str(ctx, whisper_full_get_token_data(ctx, i, j).id);
                ++j;
            }
            if (t.rfind("[_", 0) == 0) continue;
            p = seen ? (d.p < p ? d.p : p) : d.p;
            seen = true;
        }
        word(user, text, t0, t1, p);
    }
    whisper_free(ctx);
    return 0;
}

}  // extern "C"
