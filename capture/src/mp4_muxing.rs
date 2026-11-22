//! [`MiseEnMp4DuMac`] : un média importé de la famille MP4 (`.mov`, `.m4a`, `.m4v`) passé en
//! `.mp4` par `AVAssetExportSession` sans réencodage (préréglage « passthrough ») : les pistes
//! sont recopiées telles quelles dans un conteneur MP4. L'original n'est que lu.

use std::path::Path;
use std::sync::mpsc;

use block2::RcBlock;
use greffier_core::adapters::Mp4Muxer;
use objc2::AllocAnyThread;
use objc2_av_foundation::{AVAssetExportPresetPassthrough, AVAssetExportSession, AVAssetExportSessionStatus, AVFileTypeMPEG4, AVURLAsset};
use objc2_foundation::{NSString, NSURL};

pub struct MacMp4Muxer;

impl Mp4Muxer for MacMp4Muxer {
    fn to_mp4(&self, from: &Path, to: &Path) -> Result<(), String> {
        let url = |p: &Path| p.to_str().map(|s| NSURL::fileURLWithPath(&NSString::from_str(s))).ok_or_else(|| format!("chemin illisible : {}", p.display()));
        let (source, target) = (url(from)?, url(to)?);
        // SAFETY : objets AVFoundation créés et gardés ici ; le bloc ne capture qu'un Sender, et
        // on attend sa fin avant de lire l'état de la session.
        unsafe {
            let asset = AVURLAsset::URLAssetWithURL_options(&source, None);
            let session = AVAssetExportSession::initWithAsset_presetName(AVAssetExportSession::alloc(), &asset, AVAssetExportPresetPassthrough)
                .ok_or_else(|| format!("{} : AVFoundation ne sait pas le recopier", from.display()))?;
            session.setOutputURL(Some(&target));
            session.setOutputFileType(AVFileTypeMPEG4);
            let (on_done, wait) = mpsc::channel::<()>();
            let block = RcBlock::new(move || {
                let _ = on_done.send(());
            });
            session.exportAsynchronouslyWithCompletionHandler(&block);
            wait.recv().map_err(|_| "mise en MP4 interrompue".to_string())?;
            if session.status() == AVAssetExportSessionStatus::Completed {
                Ok(())
            } else {
                Err(session.error().map_or_else(|| format!("{} : mise en MP4 impossible", from.display()), |e| e.localizedDescription().to_string()))
            }
        }
    }
}
