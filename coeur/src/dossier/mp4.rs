//! La durée d'un enregistrement MP4, lue dans ses boîtes sans décoder le média : `moov/mvhd`,
//! `moov/mvex/mehd` pour un MP4 fragmenté, et la fin du dernier fragment (`moof/traf` : `tfdt` et
//! `trun`) pour un MP4 fragmenté que l'arrêt brutal de l'app a laissé sans durée. On garde la plus
//! longue : un fichier coupé peut annoncer dans son `moov` la durée de son seul premier morceau.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// La durée en secondes, ou `None` si le fichier n'est pas un MP4 lisible ou ne la dit pas.
pub fn duree(chemin: &Path) -> Option<f64> {
    let mut f = File::open(chemin).ok()?;
    let taille = f.metadata().ok()?.len();
    let mut pos = 0;
    let mut film: Option<Film> = None;
    let mut fin = 0.0_f64;
    // Une boîte tronquée (écriture coupée) arrête la lecture : ce qui précède compte.
    while pos + 8 <= taille {
        f.seek(SeekFrom::Start(pos)).ok()?;
        let Some((longueur, nom, entete)) = lire_entete(&mut f, taille - pos) else { break };
        if &nom == b"moov" || (&nom == b"moof" && film.is_some()) {
            let mut contenu = vec![0; usize::try_from(longueur - entete).ok()?];
            f.read_exact(&mut contenu).ok()?;
            match (&nom, &mut film) {
                (b"moov", _) => film = Some(Film::lire(&contenu)?),
                (_, Some(film)) => fin = fin.max(film.fragment(&contenu)),
                _ => {}
            }
        }
        pos += longueur;
    }
    let film = film?;
    let duree = fin.max(film.duree);
    (duree > 0.0).then_some(duree)
}

/// Ce que dit le `moov` : sa durée (0 si inconnue) et, par piste, l'échelle de temps et la fin
/// des échantillons déjà rangés.
struct Film {
    duree: f64,
    /// (identifiant de piste, échelle, fin courante, durée d'échantillon par défaut du `trex`)
    pistes: Vec<(u64, u64, u64, u64)>,
}

impl Film {
    fn lire(moov: &[u8]) -> Option<Film> {
        let (_, mvhd) = boites(moov).find(|(n, _)| n == b"mvhd")?;
        let version = *mvhd.first()?;
        let (echelle, duree) = if version == 1 { (u32_a(mvhd, 20)?, u64_a(mvhd, 24)?) } else { (u32_a(mvhd, 12)?, u32_a(mvhd, 16)?) };
        let inconnue = |d: u64| d == u64::from(u32::MAX) || d == u64::MAX;
        let mvex = boites(moov).find(|(n, _)| n == b"mvex").map(|(_, b)| b);
        let duree = if duree == 0 || inconnue(duree) {
            mvex.and_then(|m| boites(m).find(|(n, _)| n == b"mehd")).and_then(|(_, mehd)| if *mehd.first()? == 1 { u64_a(mehd, 4) } else { u32_a(mehd, 4) }).unwrap_or(0)
        } else {
            duree
        };
        let duree = if echelle > 0 && !inconnue(duree) { duree as f64 / echelle as f64 } else { 0.0 };
        let mut pistes = Vec::new();
        for (_, trak) in boites(moov).filter(|(n, _)| n == b"trak") {
            let Some((_, tkhd)) = boites(trak).find(|(n, _)| n == b"tkhd") else { continue };
            let id = if *tkhd.first().unwrap_or(&0) == 1 { u32_a(tkhd, 20) } else { u32_a(tkhd, 12) };
            let mdhd = boites(trak).find(|(n, _)| n == b"mdia").and_then(|(_, mdia)| boites(mdia).find(|(n, _)| n == b"mdhd")).map(|(_, b)| b);
            let (Some(id), Some(mdhd)) = (id, mdhd) else { continue };
            let (echelle, duree) = if *mdhd.first().unwrap_or(&0) == 1 { (u32_a(mdhd, 20), u64_a(mdhd, 24)) } else { (u32_a(mdhd, 12), u32_a(mdhd, 16)) };
            let Some(echelle) = echelle.filter(|e| *e > 0) else { continue };
            let duree = duree.filter(|d| !inconnue(*d)).unwrap_or(0);
            let defaut = mvex
                .into_iter()
                .flat_map(boites)
                .filter(|(n, _)| n == b"trex")
                .find(|(_, trex)| u32_a(trex, 4) == Some(id))
                .and_then(|(_, trex)| u32_a(trex, 12))
                .unwrap_or(0);
            pistes.push((id, echelle, duree, defaut));
        }
        Some(Film { duree, pistes })
    }

    /// Avance les pistes du fragment ; rend la fin la plus tardive, en secondes.
    fn fragment(&mut self, moof: &[u8]) -> f64 {
        let mut fin = 0.0_f64;
        for (_, traf) in boites(moof).filter(|(n, _)| n == b"traf") {
            let Some((_, tfhd)) = boites(traf).find(|(n, _)| n == b"tfhd") else { continue };
            let (Some(drapeaux), Some(id)) = (u32_a(tfhd, 0).map(|v| v & 0xff_ffff), u32_a(tfhd, 4)) else { continue };
            let Some(piste) = self.pistes.iter_mut().find(|p| p.0 == id) else { continue };
            let mut i = 8;
            if drapeaux & 0x01 != 0 {
                i += 8;
            }
            if drapeaux & 0x02 != 0 {
                i += 4;
            }
            let defaut = if drapeaux & 0x08 != 0 { u32_a(tfhd, i).unwrap_or(piste.3) } else { piste.3 };
            if let Some((_, tfdt)) = boites(traf).find(|(n, _)| n == b"tfdt") {
                let base = if tfdt.first() == Some(&1) { u64_a(tfdt, 4) } else { u32_a(tfdt, 4) };
                if let Some(base) = base {
                    piste.2 = base;
                }
            }
            for (_, trun) in boites(traf).filter(|(n, _)| n == b"trun") {
                piste.2 += duree_du_trun(trun, defaut).unwrap_or(0);
            }
            fin = fin.max(piste.2 as f64 / piste.1 as f64);
        }
        fin
    }
}

/// La somme des durées des échantillons d'un `trun`.
fn duree_du_trun(trun: &[u8], defaut: u64) -> Option<u64> {
    let drapeaux = u32_a(trun, 0)? & 0xff_ffff;
    let nombre = u32_a(trun, 4)?;
    let mut i = 8;
    if drapeaux & 0x01 != 0 {
        i += 4;
    }
    if drapeaux & 0x04 != 0 {
        i += 4;
    }
    if drapeaux & 0x100 == 0 {
        return Some(nombre * defaut);
    }
    let pas = 4 * [0x100, 0x200, 0x400, 0x800].iter().filter(|b| drapeaux & **b != 0).count();
    let mut total = 0;
    for k in 0..nombre as usize {
        total += u32_a(trun, i + k * pas)?;
    }
    Some(total)
}

/// La date de création inscrite dans `moov/mvhd`, si elle y est (0 veut dire absente).
pub fn date_de_creation(chemin: &Path) -> Option<std::time::SystemTime> {
    let moov = moov(chemin)?;
    let (_, mvhd) = boites(&moov).find(|(n, _)| n == b"mvhd")?;
    let secondes = if *mvhd.first()? == 1 { u64_a(mvhd, 4)? } else { u32_a(mvhd, 4)? };
    // Les dates MP4 comptent depuis le 1er janvier 1904.
    const DE_1904_A_1970: u64 = 2_082_844_800;
    let unix = secondes.checked_sub(DE_1904_A_1970).filter(|_| secondes > 0)?;
    Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(unix))
}

/// Le contenu de la boîte `moov`.
fn moov(chemin: &Path) -> Option<Vec<u8>> {
    let mut f = File::open(chemin).ok()?;
    let taille = f.metadata().ok()?.len();
    let mut pos = 0;
    while pos + 8 <= taille {
        f.seek(SeekFrom::Start(pos)).ok()?;
        let (longueur, nom, entete) = lire_entete(&mut f, taille - pos)?;
        if &nom == b"moov" {
            let mut moov = vec![0; usize::try_from(longueur - entete).ok()?];
            f.read_exact(&mut moov).ok()?;
            return Some(moov);
        }
        pos += longueur;
    }
    None
}

/// (longueur totale, nom, longueur de l'en-tête) de la boîte qui commence ici.
fn lire_entete(f: &mut File, reste: u64) -> Option<(u64, [u8; 4], u64)> {
    let mut b = [0; 8];
    f.read_exact(&mut b).ok()?;
    let nom = [b[4], b[5], b[6], b[7]];
    let (longueur, entete) = match u32::from_be_bytes([b[0], b[1], b[2], b[3]]) {
        0 => (reste, 8),
        1 => {
            let mut l = [0; 8];
            f.read_exact(&mut l).ok()?;
            (u64::from_be_bytes(l), 16)
        }
        n => (u64::from(n), 8),
    };
    (longueur >= entete && longueur <= reste).then_some((longueur, nom, entete))
}

/// Les boîtes filles d'un contenu : (nom, contenu).
fn boites(mut b: &[u8]) -> impl Iterator<Item = ([u8; 4], &[u8])> {
    std::iter::from_fn(move || {
        if b.len() < 8 {
            return None;
        }
        let n = u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize;
        let nom = [b[4], b[5], b[6], b[7]];
        let (n, entete) = match n {
            0 => (b.len(), 8),
            1 if b.len() >= 16 => (usize::try_from(u64::from_be_bytes(b[8..16].try_into().ok()?)).ok()?, 16),
            n => (n, 8),
        };
        if n < entete || n > b.len() {
            return None;
        }
        let contenu = &b[entete..n];
        b = &b[n..];
        Some((nom, contenu))
    })
}

fn u32_a(b: &[u8], i: usize) -> Option<u64> {
    Some(u64::from(u32::from_be_bytes(b.get(i..i + 4)?.try_into().ok()?)))
}

fn u64_a(b: &[u8], i: usize) -> Option<u64> {
    Some(u64::from_be_bytes(b.get(i..i + 8)?.try_into().ok()?))
}
