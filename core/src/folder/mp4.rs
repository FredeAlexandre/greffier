//! An MP4 recording's duration, read from its boxes without decoding the media: `moov/mvhd`,
//! `moov/mvex/mehd` for a fragmented MP4, and the end of the last fragment (`moof/traf`: `tfdt` and
//! `trun`) for a fragmented MP4 that the app's abrupt stop left without a duration. The longest is
//! kept: a cut-off file may announce in its `moov` the duration of its first chunk only.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// The duration in seconds, or `None` if the file is not a readable MP4 or does not state it.
pub fn duration(path: &Path) -> Option<f64> {
    let mut f = File::open(path).ok()?;
    let size = f.metadata().ok()?.len();
    let mut pos = 0;
    let mut movie: Option<Movie> = None;
    let mut end = 0.0_f64;
    // A truncated box (interrupted write) stops reading: what comes before counts.
    while pos + 8 <= size {
        f.seek(SeekFrom::Start(pos)).ok()?;
        let Some((length, name, header)) = read_header(&mut f, size - pos) else { break };
        if &name == b"moov" || (&name == b"moof" && movie.is_some()) {
            let mut content = vec![0; usize::try_from(length - header).ok()?];
            f.read_exact(&mut content).ok()?;
            match (&name, &mut movie) {
                (b"moov", _) => movie = Some(Movie::read(&content)?),
                (_, Some(movie)) => end = end.max(movie.fragment(&content)),
                _ => {}
            }
        }
        pos += length;
    }
    let movie = movie?;
    let duration = end.max(movie.duration);
    (duration > 0.0).then_some(duration)
}

/// What the `moov` says: its duration (0 if unknown) and, per track, the timescale and the end
/// of the samples already stored.
struct Movie {
    duration: f64,
    /// (track id, timescale, current end, the `trex`'s default sample duration)
    tracks: Vec<(u64, u64, u64, u64)>,
}

impl Movie {
    fn read(moov: &[u8]) -> Option<Movie> {
        let (_, mvhd) = boxes(moov).find(|(n, _)| n == b"mvhd")?;
        let version = *mvhd.first()?;
        let (timescale, duration) = if version == 1 { (u32_a(mvhd, 20)?, u64_a(mvhd, 24)?) } else { (u32_a(mvhd, 12)?, u32_a(mvhd, 16)?) };
        let unknown = |d: u64| d == u64::from(u32::MAX) || d == u64::MAX;
        let mvex = boxes(moov).find(|(n, _)| n == b"mvex").map(|(_, b)| b);
        let duration = if duration == 0 || unknown(duration) {
            mvex.and_then(|m| boxes(m).find(|(n, _)| n == b"mehd")).and_then(|(_, mehd)| if *mehd.first()? == 1 { u64_a(mehd, 4) } else { u32_a(mehd, 4) }).unwrap_or(0)
        } else {
            duration
        };
        let duration = if timescale > 0 && !unknown(duration) { duration as f64 / timescale as f64 } else { 0.0 };
        let mut tracks = Vec::new();
        for (_, trak) in boxes(moov).filter(|(n, _)| n == b"trak") {
            let Some((_, tkhd)) = boxes(trak).find(|(n, _)| n == b"tkhd") else { continue };
            let id = if *tkhd.first().unwrap_or(&0) == 1 { u32_a(tkhd, 20) } else { u32_a(tkhd, 12) };
            let mdhd = boxes(trak).find(|(n, _)| n == b"mdia").and_then(|(_, mdia)| boxes(mdia).find(|(n, _)| n == b"mdhd")).map(|(_, b)| b);
            let (Some(id), Some(mdhd)) = (id, mdhd) else { continue };
            let (timescale, duration) = if *mdhd.first().unwrap_or(&0) == 1 { (u32_a(mdhd, 20), u64_a(mdhd, 24)) } else { (u32_a(mdhd, 12), u32_a(mdhd, 16)) };
            let Some(timescale) = timescale.filter(|e| *e > 0) else { continue };
            let duration = duration.filter(|d| !unknown(*d)).unwrap_or(0);
            let default = mvex
                .into_iter()
                .flat_map(boxes)
                .filter(|(n, _)| n == b"trex")
                .find(|(_, trex)| u32_a(trex, 4) == Some(id))
                .and_then(|(_, trex)| u32_a(trex, 12))
                .unwrap_or(0);
            tracks.push((id, timescale, duration, default));
        }
        Some(Movie { duration, tracks })
    }

    /// Advances the fragment's tracks; returns the latest end, in seconds.
    fn fragment(&mut self, moof: &[u8]) -> f64 {
        let mut end = 0.0_f64;
        for (_, traf) in boxes(moof).filter(|(n, _)| n == b"traf") {
            let Some((_, tfhd)) = boxes(traf).find(|(n, _)| n == b"tfhd") else { continue };
            let (Some(flags), Some(id)) = (u32_a(tfhd, 0).map(|v| v & 0xff_ffff), u32_a(tfhd, 4)) else { continue };
            let Some(track) = self.tracks.iter_mut().find(|p| p.0 == id) else { continue };
            let mut i = 8;
            if flags & 0x01 != 0 {
                i += 8;
            }
            if flags & 0x02 != 0 {
                i += 4;
            }
            let default = if flags & 0x08 != 0 { u32_a(tfhd, i).unwrap_or(track.3) } else { track.3 };
            if let Some((_, tfdt)) = boxes(traf).find(|(n, _)| n == b"tfdt") {
                let base = if tfdt.first() == Some(&1) { u64_a(tfdt, 4) } else { u32_a(tfdt, 4) };
                if let Some(base) = base {
                    track.2 = base;
                }
            }
            for (_, trun) in boxes(traf).filter(|(n, _)| n == b"trun") {
                track.2 += trun_duration(trun, default).unwrap_or(0);
            }
            end = end.max(track.2 as f64 / track.1 as f64);
        }
        end
    }
}

/// The sum of a `trun`'s sample durations.
fn trun_duration(trun: &[u8], default: u64) -> Option<u64> {
    let flags = u32_a(trun, 0)? & 0xff_ffff;
    let number = u32_a(trun, 4)?;
    let mut i = 8;
    if flags & 0x01 != 0 {
        i += 4;
    }
    if flags & 0x04 != 0 {
        i += 4;
    }
    if flags & 0x100 == 0 {
        return Some(number * default);
    }
    let step = 4 * [0x100, 0x200, 0x400, 0x800].iter().filter(|b| flags & **b != 0).count();
    let mut total = 0;
    for k in 0..number as usize {
        total += u32_a(trun, i + k * step)?;
    }
    Some(total)
}

/// The creation date stored in `moov/mvhd`, if present (0 means absent).
pub fn creation_date(path: &Path) -> Option<std::time::SystemTime> {
    let moov = moov(path)?;
    let (_, mvhd) = boxes(&moov).find(|(n, _)| n == b"mvhd")?;
    let seconds = if *mvhd.first()? == 1 { u64_a(mvhd, 4)? } else { u32_a(mvhd, 4)? };
    // MP4 dates count from January 1, 1904.
    const FROM_1904_TO_1970: u64 = 2_082_844_800;
    let unix = seconds.checked_sub(FROM_1904_TO_1970).filter(|_| seconds > 0)?;
    Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(unix))
}

/// The content of the `moov` box.
fn moov(path: &Path) -> Option<Vec<u8>> {
    let mut f = File::open(path).ok()?;
    let size = f.metadata().ok()?.len();
    let mut pos = 0;
    while pos + 8 <= size {
        f.seek(SeekFrom::Start(pos)).ok()?;
        let (length, name, header) = read_header(&mut f, size - pos)?;
        if &name == b"moov" {
            let mut moov = vec![0; usize::try_from(length - header).ok()?];
            f.read_exact(&mut moov).ok()?;
            return Some(moov);
        }
        pos += length;
    }
    None
}

/// (total length, name, header length) of the box starting here.
fn read_header(f: &mut File, rest: u64) -> Option<(u64, [u8; 4], u64)> {
    let mut b = [0; 8];
    f.read_exact(&mut b).ok()?;
    let name = [b[4], b[5], b[6], b[7]];
    let (length, header) = match u32::from_be_bytes([b[0], b[1], b[2], b[3]]) {
        0 => (rest, 8),
        1 => {
            let mut l = [0; 8];
            f.read_exact(&mut l).ok()?;
            (u64::from_be_bytes(l), 16)
        }
        n => (u64::from(n), 8),
    };
    (length >= header && length <= rest).then_some((length, name, header))
}

/// The child boxes of a content: (name, content).
fn boxes(mut b: &[u8]) -> impl Iterator<Item = ([u8; 4], &[u8])> {
    std::iter::from_fn(move || {
        if b.len() < 8 {
            return None;
        }
        let n = u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize;
        let name = [b[4], b[5], b[6], b[7]];
        let (n, header) = match n {
            0 => (b.len(), 8),
            1 if b.len() >= 16 => (usize::try_from(u64::from_be_bytes(b[8..16].try_into().ok()?)).ok()?, 16),
            n => (n, 8),
        };
        if n < header || n > b.len() {
            return None;
        }
        let content = &b[header..n];
        b = &b[n..];
        Some((name, content))
    })
}

fn u32_a(b: &[u8], i: usize) -> Option<u64> {
    Some(u64::from(u32::from_be_bytes(b.get(i..i + 4)?.try_into().ok()?)))
}

fn u64_a(b: &[u8], i: usize) -> Option<u64> {
    Some(u64::from_be_bytes(b.get(i..i + 8)?.try_into().ok()?))
}
