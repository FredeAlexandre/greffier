//! `cargo xtask vendor-whisper`: copies into crates/engines/whisper.cpp/ the whisper.cpp sources Greffier
//! needs (whisper, ggml, CPU, Metal, Accelerate's BLAS), from the archive of the version pinned in
//! crates/engines/whisper-version.rs. crates/engines/build.rs compiles them without cmake.

use std::path::Path;

use crate::{Failure, Result, TempDir, download, files_under, root, run, step};

#[allow(dead_code)]
mod pinned {
    include!("../../crates/engines/whisper-version.rs");
}
use pinned::{WHISPER_ARCHIVE_SHA256, WHISPER_VERSION};

pub fn main(args: &[String]) -> Result<()> {
    if let Some(arg) = args.first() {
        return Err(Failure(format!("unexpected argument \"{arg}\" (the version is pinned in crates/engines/whisper-version.rs).")));
    }
    let target = root().join("crates/engines/whisper.cpp");
    let staging = TempDir::new("greffier-whisper-")?;
    let archive = staging.path().join("whisper.tar.gz");
    step(&format!("Downloading whisper.cpp {WHISPER_VERSION}"));
    download(&format!("https://github.com/ggml-org/whisper.cpp/archive/refs/tags/v{WHISPER_VERSION}.tar.gz"), &archive, WHISPER_ARCHIVE_SHA256)?;
    run("tar", &["-xzf".as_ref(), archive.as_os_str(), "-C".as_ref(), staging.path().as_os_str()])?;
    let source = staging.path().join(format!("whisper.cpp-{WHISPER_VERSION}"));

    let kept: Vec<_> = files_under(&source)?.into_iter().filter(|path| needed(&path.to_string_lossy())).collect();
    let _ = std::fs::remove_dir_all(&target);
    for path in &kept {
        let dest = target.join(path);
        std::fs::create_dir_all(dest.parent().unwrap())?;
        std::fs::copy(source.join(path), dest)?;
    }
    println!("whisper.cpp {WHISPER_VERSION} → {} ({} files)", target.display(), kept.len());
    Ok(())
}

/// Whether Greffier's build needs this file of the archive (path relative to its root).
fn needed(path: &str) -> bool {
    let extension = Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or_default();
    let name = Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let directly_in = |dir: &str| path.strip_prefix(dir).is_some_and(|rest| !rest.contains('/'));
    match path {
        "LICENSE" | "include/whisper.h" | "ggml/src/ggml-blas/ggml-blas.cpp" => true,
        _ if directly_in("src/") => name.starts_with("whisper"),
        _ if path.starts_with("src/aneforge/") => true,
        _ if path.starts_with("ggml/include/") => extension == "h",
        _ if directly_in("ggml/src/") => matches!(extension, "c" | "cpp" | "h" | "in"),
        _ if path.starts_with("ggml/src/ggml-cpu/") => {
            let excluded = ["/kleidiai/", "/spacemit/", "/cmake/"].iter().any(|dir| path.contains(dir));
            let other_arch = path.contains("/arch/") && !path.contains("/arch/arm/");
            matches!(extension, "c" | "cpp" | "h") && !excluded && !other_arch
        }
        _ if path.starts_with("ggml/src/ggml-metal/") => matches!(extension, "c" | "cpp" | "h" | "m" | "metal") || name == "CMakeLists.txt",
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::needed;

    #[test]
    fn keeps_whisper_ggml_cpu_for_arm_metal_and_blas() {
        for path in [
            "LICENSE",
            "include/whisper.h",
            "src/whisper.cpp",
            "src/whisper-arch.h",
            "src/aneforge/whisper-aneforge.cpp",
            "ggml/include/ggml.h",
            "ggml/src/ggml.c",
            "ggml/src/ggml-version.h.in",
            "ggml/src/ggml-cpu/ops.cpp",
            "ggml/src/ggml-cpu/arch/arm/quants.c",
            "ggml/src/ggml-metal/kernels/rope.metal",
            "ggml/src/ggml-metal/CMakeLists.txt",
            "ggml/src/ggml-blas/ggml-blas.cpp",
        ] {
            assert!(needed(path), "{path}");
        }
    }

    #[test]
    fn leaves_out_other_backends_architectures_and_tooling() {
        for path in [
            "README.md",
            "CMakeLists.txt",
            "src/CMakeLists.txt",
            "src/coreml/whisper-encoder.mm",
            "examples/cli/cli.cpp",
            "ggml/src/CMakeLists.txt",
            "ggml/src/ggml-cuda/ggml-cuda.cu",
            "ggml/src/ggml-cpu/arch/x86/quants.c",
            "ggml/src/ggml-cpu/kleidiai/kleidiai.cpp",
            "ggml/src/ggml-cpu/spacemit/ime.cpp",
            "ggml/src/ggml-cpu/cmake/FindSIMD.cmake",
            "ggml/src/ggml-cpu/CMakeLists.txt",
            "ggml/src/ggml-blas/CMakeLists.txt",
        ] {
            assert!(!needed(path), "{path}");
        }
    }
}
