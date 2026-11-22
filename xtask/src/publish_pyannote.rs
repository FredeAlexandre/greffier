//! `cargo xtask publish-pyannote <folder>`: publishes the pyannote speaker-diarization-community-1
//! files (CC-BY-4.0) as a GitHub release of the repository, with the license and the attribution,
//! so that Greffier downloads them without a Hugging Face account. Run once, from a local copy of
//! the model (revision 3533c8c…).
//!
//! The files, their names in the release, the release's tag and the expected sizes and sha256s all
//! come from the core's catalog (core/src/resources.rs); a file that does not match is not published.

use std::path::{Path, PathBuf};

use greffier_core::resources::{Catalogue, Content, PYANNOTE_MODEL, ResourceFile};

use crate::{Failure, Result, TempDir, check_sha256, root, run, step};

pub fn main(args: &[String]) -> Result<()> {
    let [source] = args else {
        return Err(Failure("usage: cargo xtask publish-pyannote <pyannote-community-1 folder>".into()));
    };
    let release = Release::from_catalogue(&Catalogue::for_greffier())?;
    let staging = TempDir::new("greffier-pyannote-")?;
    let mut assets: Vec<PathBuf> = Vec::new();
    for asset in &release.assets {
        let from = Path::new(source).join(&asset.in_model);
        step(&format!("Checking {}", asset.in_model));
        let size = std::fs::metadata(&from).map_err(|e| Failure(format!("{}: {e}", from.display())))?.len();
        if size != asset.size {
            return Err(Failure(format!("unexpected size for {}: {size} bytes (expected {}).", asset.in_model, asset.size)));
        }
        check_sha256(&from, &asset.sha256)?;
        let to = staging.path().join(&asset.name);
        std::fs::copy(&from, &to)?;
        assets.push(to);
    }
    let notes = root().join("xtask/pyannote/ATTRIBUTION");
    for extra in [notes.clone(), root().join("xtask/pyannote/LICENSE")] {
        let to = staging.path().join(extra.file_name().unwrap());
        std::fs::copy(&extra, &to)?;
        assets.push(to);
    }
    step(&format!("Creating the release {} of {}", release.tag, release.repo));
    let mut args: Vec<String> = vec!["release".into(), "create".into(), release.tag.clone(), "--repo".into(), release.repo.clone(), "--target".into(), "main".into()];
    args.extend(["--title".into(), "pyannote speaker-diarization-community-1 (CC-BY-4.0)".into(), "--notes-file".into(), notes.to_string_lossy().into_owned()]);
    args.extend(assets.iter().map(|a| a.to_string_lossy().into_owned()));
    let output = run("gh", &args)?;
    print!("{}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}

/// The GitHub release the catalog downloads pyannote from.
#[derive(Debug, PartialEq)]
struct Release {
    /// `owner/name`.
    repo: String,
    tag: String,
    assets: Vec<Asset>,
}

#[derive(Debug, PartialEq)]
struct Asset {
    /// Path in the model's folder.
    in_model: String,
    /// Name in the release.
    name: String,
    size: u64,
    sha256: String,
}

impl Release {
    fn from_catalogue(catalogue: &Catalogue) -> Result<Release> {
        let prefix = format!("{PYANNOTE_MODEL}/");
        let files: Vec<&ResourceFile> = catalogue
            .0
            .iter()
            .filter_map(|element| match &element.content {
                Content::Files(files) => Some(files),
                _ => None,
            })
            .flatten()
            .filter(|file| file.path.starts_with(&prefix))
            .collect();
        let mut release: Option<Release> = None;
        for file in files {
            let (base, name) = file.url.rsplit_once('/').ok_or(Failure(format!("no file name in {}", file.url)))?;
            let (repo, tag) = base
                .strip_prefix("https://github.com/")
                .and_then(|rest| rest.split_once("/releases/download/"))
                .ok_or(Failure(format!("{} is not a GitHub release download.", file.url)))?;
            let release = release.get_or_insert_with(|| Release { repo: repo.into(), tag: tag.into(), assets: Vec::new() });
            if (release.repo.as_str(), release.tag.as_str()) != (repo, tag) {
                return Err(Failure(format!("the pyannote files come from several releases ({} and {repo} {tag}).", release.tag)));
            }
            release.assets.push(Asset { in_model: file.path[prefix.len()..].into(), name: name.into(), size: file.size, sha256: file.sha256.clone() });
        }
        release.ok_or(Failure(format!("the catalog has no file under {PYANNOTE_MODEL}.")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_release_is_the_one_greffier_downloads_from() {
        let release = Release::from_catalogue(&Catalogue::for_greffier()).unwrap();
        assert_eq!(release.repo, "FredeAlexandre/greffier");
        assert_eq!(release.tag, "pyannote-community-1");
        let names: Vec<_> = release.assets.iter().map(|a| (a.in_model.as_str(), a.name.as_str())).collect();
        assert_eq!(names, [
            ("config.yaml", "config.yaml"),
            ("segmentation/pytorch_model.bin", "segmentation-pytorch_model.bin"),
            ("embedding/pytorch_model.bin", "embedding-pytorch_model.bin"),
            ("plda/plda.npz", "plda-plda.npz"),
            ("plda/xvec_transform.npz", "plda-xvec_transform.npz"),
        ]);
    }

    #[test]
    fn the_license_and_attribution_ship_with_it() {
        assert!(root().join("xtask/pyannote/LICENSE").is_file());
        assert!(root().join("xtask/pyannote/ATTRIBUTION").is_file());
    }
}
