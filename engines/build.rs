//! Compile whisper.cpp (version figée, sources vendues dans `whisper.cpp/` par
//! `scripts/vendre-whisper`) sans cmake : ggml, son moteur CPU (Accelerate), son moteur Metal
//! (bibliothèque Metal embarquée dans l'exécutable) et le BLAS d'Accelerate, avec les options par
//! défaut que cmake prendrait sur un Mac Apple Silicon (GGML_NATIVE, GGML_METAL,
//! GGML_METAL_EMBED_LIBRARY, GGML_BLAS, GGML_ACCELERATE, GGML_CPU_REPACK).

use std::path::{Path, PathBuf};

const WHISPER_VERSION: &str = "1.9.5";
const GGML_VERSION: &str = "0.26.0";

/// Les bibliothèques Metal, une par noyau (ordre de `GGML_METAL_LIBS`, ggml-metal-device.m).
const METAL_KERNELS: &[&str] = &[
    "fa_aux", "fa_f16", "fa_f32", "fa_q4_0", "fa_q4_1", "fa_q5_0", "fa_q5_1", "fa_q8_0", "fa_vec_f16", "fa_vec_f32", "fa_vec_q4_0", "fa_vec_q4_1", "fa_vec_q5_0",
    "fa_vec_q5_1", "fa_vec_q8_0", "mul_mv", "mul_mv_mma", "mul_mm", "quantize", "softmax", "norm", "unary", "binbcast", "reduce", "tri", "ssm", "wkv", "gated_delta_net",
    "solve_tri", "rope", "conv", "upscale", "argsort", "pool", "misc",
];

fn main() {
    let w = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("whisper.cpp");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    println!("cargo:rerun-if-changed=whisper.cpp");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/transcribe.cpp");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        panic!("greffier-moteurs ne se construit que sur macOS");
    }

    let g = w.join("ggml");
    let src = g.join("src");
    std::fs::write(out.join("ggml-version.h"), format!("#pragma once\n#define GGML_VERSION \"{GGML_VERSION}\"\n#define GGML_COMMIT \"v{WHISPER_VERSION}\"\n")).unwrap();

    // Commun à tout : conformité POSIX de ggml/src/CMakeLists.txt.
    let base = |cpp: bool| {
        let mut b = cc::Build::new();
        b.cpp(cpp)
            .opt_level(3)
            .debug(false)
            .warnings(false)
            .define("NDEBUG", None)
            .define("_XOPEN_SOURCE", "600")
            .define("_DARWIN_C_SOURCE", None)
            .define("GGML_SCHED_MAX_COPIES", "4")
            .include(g.join("include"))
            .include(&src)
            .include(&out)
            .flag("-mcpu=native");
        if cpp {
            b.std("c++17");
        } else {
            b.std("c11");
        }
        b
    };
    let configure = |b: &mut cc::Build| {
        b.define("GGML_USE_CPU", None).define("GGML_USE_METAL", None).define("GGML_USE_BLAS", None);
    };

    // ggml-base et ggml.
    let mut c = base(false);
    c.files(["ggml.c", "ggml-alloc.c", "ggml-quants.c"].map(|f| src.join(f)));
    c.compile("ggml-base-c");
    let mut cpp = base(true);
    cpp.files(["ggml.cpp", "ggml-backend.cpp", "ggml-backend-meta.cpp", "ggml-opt.cpp", "ggml-threading.cpp", "gguf.cpp"].map(|f| src.join(f)));
    cpp.compile("ggml-base-cpp");
    let mut reg = base(true);
    configure(&mut reg);
    reg.files(["ggml-backend-reg.cpp", "ggml-backend-dl.cpp"].map(|f| src.join(f)));
    reg.compile("ggml-reg");

    // Moteur CPU.
    let cpu = src.join("ggml-cpu");
    let cpu_options = |b: &mut cc::Build| {
        b.include(&cpu)
            .define("GGML_USE_ACCELERATE", None)
            .define("ACCELERATE_NEW_LAPACK", None)
            .define("ACCELERATE_LAPACK_ILP64", None)
            .define("GGML_USE_CPU_REPACK", None);
    };
    let mut c = base(false);
    cpu_options(&mut c);
    c.files(["ggml-cpu.c", "quants.c", "arch/arm/quants.c"].map(|f| cpu.join(f)));
    c.compile("ggml-cpu-c");
    let mut cpp = base(true);
    cpu_options(&mut cpp);
    cpp.files(
        [
            "ggml-cpu.cpp",
            "repack.cpp",
            "hbm.cpp",
            "traits.cpp",
            "amx/amx.cpp",
            "amx/mmq.cpp",
            "tiled/tiled.cpp",
            "tiled/tiled-kernel.cpp",
            "binary-ops.cpp",
            "unary-ops.cpp",
            "vec.cpp",
            "ops.cpp",
            "arch/arm/repack.cpp",
        ]
        .map(|f| cpu.join(f)),
    );
    cpp.compile("ggml-cpu-cpp");

    // BLAS d'Accelerate.
    let mut blas = base(true);
    blas.define("GGML_BLAS_USE_ACCELERATE", None).define("ACCELERATE_NEW_LAPACK", None).define("ACCELERATE_LAPACK_ILP64", None).file(src.join("ggml-blas/ggml-blas.cpp"));
    blas.compile("ggml-blas");

    // Moteur Metal, avec ses noyaux embarqués.
    let metal = src.join("ggml-metal");
    let mut m = base(false);
    m.define("GGML_METAL_EMBED_LIBRARY", None).flag("-fno-objc-arc").files(["ggml-metal-device.m", "ggml-metal-context.m"].map(|f| metal.join(f)));
    m.compile("ggml-metal-objc");
    let mut cpp = base(true);
    cpp.define("GGML_METAL_EMBED_LIBRARY", None).files(
        ["ggml-metal.cpp", "ggml-metal-device.cpp", "ggml-metal-common.cpp", "ggml-metal-fusion.cpp", "ggml-metal-ops.cpp", "ggml-metal-tuning.cpp"].map(|f| metal.join(f)),
    );
    cpp.compile("ggml-metal-cpp");
    let mut asm = cc::Build::new();
    asm.flag("-Wno-unused-command-line-argument");
    for kernel in METAL_KERNELS {
        let s = embedded_kernel(&metal, &src, &out, kernel);
        asm.file(s);
    }
    asm.compile("ggml-metal-noyaux");

    // whisper.
    let mut wh = base(true);
    configure(&mut wh);
    wh.define("WHISPER_VERSION", format!("\"{WHISPER_VERSION}\"").as_str());
    wh.include(w.join("include")).include(w.join("src")).files([w.join("src/whisper.cpp"), w.join("src/aneforge/whisper-aneforge.cpp")]);
    wh.file(PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("src/transcribe.cpp"));
    wh.compile("whisper");

    for f in ["Foundation", "Metal", "MetalKit", "Accelerate"] {
        println!("cargo:rustc-link-lib=framework={f}");
    }
    println!("cargo:rustc-link-lib=c++");
}

/// Le source Metal d'un noyau, en-têtes inclus, et l'assembleur qui l'embarque entre les
/// symboles `ggml_metallib_<noyau>_{start,end}` (les étapes de ggml-metal/CMakeLists.txt).
fn embedded_kernel(metal: &Path, src: &Path, out: &Path, kernel: &str) -> PathBuf {
    let read = |p: PathBuf| std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{} : {e}", p.display()));
    let k = metal.join("kernels");
    let source = read(k.join(format!("{kernel}.metal")));
    let mut text = read(k.join("common.h"));
    let includes = |name: &str| source.lines().any(|l| l.trim() == format!("#include \"{name}\""));
    for (name, file) in [("dequantize.h", "dequantize.h"), ("quantize.h", "quantize.h"), ("fa_common.metal", "fa_common.metal"), ("fa_vec_common.metal", "fa_vec_common.metal")] {
        if includes(name) {
            text.push_str(&read(k.join(file)));
        }
    }
    text.push_str(&source);
    let common = read(src.join("ggml-common.h"));
    let implementation = read(metal.join("ggml-metal-impl.h"));
    let removed = ["#include \"common.h\"", "#include \"dequantize.h\"", "#include \"quantize.h\"", "#include \"fa_common.metal\"", "#include \"fa_vec_common.metal\"", "#pragma once"];
    let mut final_ = String::new();
    for line in text.split_inclusive('\n') {
        if removed.iter().any(|r| line.contains(r)) {
            continue;
        }
        if line.contains("__embed_ggml-common.h__") {
            final_.push_str(&common);
            continue;
        }
        if line.contains("#include \"ggml-metal-impl.h\"") {
            final_.push_str(&implementation);
            continue;
        }
        final_.push_str(line);
    }
    let embedded = out.join(format!("ggml-metal-embed-{kernel}.metal"));
    std::fs::write(&embedded, final_).unwrap();
    let s = out.join(format!("ggml-metal-embed-{kernel}.s"));
    std::fs::write(
        &s,
        format!(
            ".section __DATA,__ggml_metallib\n.globl _ggml_metallib_{kernel}_start\n_ggml_metallib_{kernel}_start:\n.incbin \"{}\"\n.globl _ggml_metallib_{kernel}_end\n_ggml_metallib_{kernel}_end:\n",
            embedded.display()
        ),
    )
    .unwrap();
    s
}
