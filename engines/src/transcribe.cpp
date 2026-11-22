// Une transcription whisper.cpp avec les réglages de la commande du vault :
//
//     whisper-cli -m ggml-large-v3.bin -f audio.wav -l fr -t 6 \
//       [--prompt "<vocabulaire>" --carry-initial-prompt] -ml 1 -sow -ojf \
//       --vad -vm ggml-silero-v5.1.2.bin -pp -np
//
// Les paramètres sont posés exactement comme examples/cli/cli.cpp les pose (valeurs par défaut de
// whisper-cli pour tout le reste : beam 5, best-of 5, température 0 avec repli de 0,2…). Chaque
// segment (un mot, grâce à -ml 1 -sow) est rendu avec sa confiance : le minimum des `p` de ses
// jetons, sans les jetons spéciaux (« [_BEG_] », « [_TT_…] »), comme le lisait le vault dans le
// JSON complet (jetons UTF-8 coupés regroupés, `p` du premier).

#include "whisper.h"

#include <atomic>
#include <cfloat>
#include <cstring>
#include <string>

extern "C" {

struct greffier_reglages {
    const char * modele;
    const char * vad;
    const char * langue;
    int threads;
    // Vide : pas de prompt.
    const char * vocabulaire;
};

typedef void (*greffier_mot)(void * user, const char * texte, int64_t t0_ms, int64_t t1_ms, float p);
typedef void (*greffier_progres)(void * user, int progres);
typedef bool (*greffier_abandon)(void * user);

static std::string derniere_erreur;

static void journal(enum ggml_log_level niveau, const char * texte, void *) {
    if (niveau == GGML_LOG_LEVEL_ERROR && texte) {
        derniere_erreur = texte;
    }
}

// utf8_trailing_bytes_needed d'examples/common-whisper.cpp.
static int octets_utf8_manquants(const std::string & s) {
    const int n = (int) s.size();
    int i = n - 1;
    while (i >= 0 && ((unsigned char) s[i] & 0xC0) == 0x80) {
        --i;
    }
    if (i < 0) {
        return 0;
    }
    const unsigned char c = (unsigned char) s[i];
    int attendu;
    if ((c & 0x80) == 0x00) {
        attendu = 1;
    } else if ((c & 0xE0) == 0xC0) {
        attendu = 2;
    } else if ((c & 0xF0) == 0xE0) {
        attendu = 3;
    } else if ((c & 0xF8) == 0xF0) {
        attendu = 4;
    } else {
        return 0;
    }
    const int present = n - i;
    return present >= attendu ? 0 : (attendu - present);
}

struct rappels {
    void * user;
    greffier_progres progres;
    greffier_abandon abandon;
};

// 0 : fini ; 1 : modèle illisible ; 2 : échec de whisper ; 3 : abandon.
int greffier_transcribe(const greffier_reglages * r, const float * pcm, int n, void * user, greffier_mot mot, greffier_progres progres, greffier_abandon abandon,
                        char * erreur, int taille_erreur) {
    whisper_log_set(journal, nullptr);
    derniere_erreur.clear();
    auto rendre_erreur = [&](const char * defaut) {
        const std::string & m = derniere_erreur.empty() ? std::string(defaut) : derniere_erreur;
        if (taille_erreur > 0) {
            std::strncpy(erreur, m.c_str(), taille_erreur - 1);
            erreur[taille_erreur - 1] = 0;
        }
    };

    whisper_context_params cparams = whisper_context_default_params();
    cparams.use_gpu = true;
    cparams.gpu_device = 0;
    cparams.flash_attn = true;
    whisper_context * ctx = whisper_init_from_file_with_params(r->modele, cparams);
    if (!ctx) {
        rendre_erreur("impossible de charger le modèle");
        return 1;
    }

    const bool avec_vocabulaire = r->vocabulaire && r->vocabulaire[0];

    whisper_full_params w = whisper_full_default_params(WHISPER_SAMPLING_GREEDY);
    const int beam_size = whisper_full_default_params(WHISPER_SAMPLING_BEAM_SEARCH).beam_search.beam_size;
    const int best_of = whisper_full_default_params(WHISPER_SAMPLING_GREEDY).greedy.best_of;
    w.strategy = beam_size > 1 ? WHISPER_SAMPLING_BEAM_SEARCH : WHISPER_SAMPLING_GREEDY;
    w.print_realtime = false;
    w.print_progress = false;  // -pp : l'avancement passe par le rappel
    w.print_timestamps = true;
    w.print_special = false;
    w.translate = false;
    w.language = r->langue;
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
    w.initial_prompt = avec_vocabulaire ? r->vocabulaire : "";
    w.carry_initial_prompt = avec_vocabulaire;
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

    rappels rp{user, progres, abandon};
    w.progress_callback = [](whisper_context *, whisper_state *, int p, void * u) {
        auto * rp = (rappels *) u;
        rp->progres(rp->user, p);
    };
    w.progress_callback_user_data = &rp;
    w.abort_callback = [](void * u) {
        auto * rp = (rappels *) u;
        return rp->abandon(rp->user);
    };
    w.abort_callback_user_data = &rp;
    w.encoder_begin_callback = [](whisper_context *, whisper_state *, void * u) {
        auto * rp = (rappels *) u;
        return !rp->abandon(rp->user);
    };
    w.encoder_begin_callback_user_data = &rp;

    const int res = whisper_full(ctx, w, pcm, n);
    if (abandon(user)) {
        whisper_free(ctx);
        return 3;
    }
    if (res != 0) {
        rendre_erreur("whisper a échoué");
        whisper_free(ctx);
        return 2;
    }

    const int n_segments = whisper_full_n_segments(ctx);
    for (int i = 0; i < n_segments; ++i) {
        const char * texte = whisper_full_get_segment_text(ctx, i);
        const int64_t t0 = whisper_full_get_segment_t0(ctx, i) * 10;
        const int64_t t1 = whisper_full_get_segment_t1(ctx, i) * 10;
        float p = 1.0f;
        bool vu = false;
        const int nt = whisper_full_n_tokens(ctx, i);
        for (int j = 0; j < nt;) {
            whisper_token_data d = whisper_full_get_token_data(ctx, i, j);
            std::string t = whisper_token_to_str(ctx, d.id);
            ++j;
            while (j < nt && octets_utf8_manquants(t) > 0) {
                t += whisper_token_to_str(ctx, whisper_full_get_token_data(ctx, i, j).id);
                ++j;
            }
            if (t.rfind("[_", 0) == 0) continue;
            p = vu ? (d.p < p ? d.p : p) : d.p;
            vu = true;
        }
        mot(user, texte, t0, t1, p);
    }
    whisper_free(ctx);
    return 0;
}

}  // extern "C"
