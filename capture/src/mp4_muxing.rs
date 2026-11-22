//! [`MacMp4Muxer`]: an imported medium from the MP4 family (`.mov`, `.m4a`, `.m4v`) turned into
//! `.mp4` by `AVAssetExportSession` without re-encoding ("passthrough" preset): the tracks are
//! copied as is into an MP4 container. The original is only read.

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
        // SAFETY: AVFoundation objects created and held here; the block captures only a Sender,
        // and we wait for it to finish before reading the session's status.
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
