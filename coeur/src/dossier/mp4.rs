//! La durée d'un enregistrement MP4, lue dans ses boîtes (`moov/mvhd`, ou `moov/mvex/mehd` pour
//! un MP4 fragmenté) sans décoder le média.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// La durée en secondes, ou `None` si le fichier n'est pas un MP4 lisible ou ne la dit pas.
pub fn duree(chemin: &Path) -> Option<f64> {
    let mut f = File::open(chemin).ok()?;
    let taille = f.metadata().ok()?.len();
    let mut pos = 0;
    while pos + 8 <= taille {
        f.seek(SeekFrom::Start(pos)).ok()?;
        let (longueur, nom, entete) = lire_entete(&mut f, taille - pos)?;
        if &nom == b"moov" {
            let mut moov = vec![0; usize::try_from(longueur - entete).ok()?];
            f.read_exact(&mut moov).ok()?;
            return duree_du_moov(&moov);
        }
        pos += longueur;
    }
    None
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

fn duree_du_moov(moov: &[u8]) -> Option<f64> {
    let (_, mvhd) = boites(moov).find(|(n, _)| n == b"mvhd")?;
    let version = *mvhd.first()?;
    let (echelle, duree) = if version == 1 { (u32_a(mvhd, 20)?, u64_a(mvhd, 24)?) } else { (u32_a(mvhd, 12)?, u32_a(mvhd, 16)?) };
    let inconnue = duree == 0 || duree == u64::from(u32::MAX) || duree == u64::MAX;
    let duree = if inconnue {
        let (_, mvex) = boites(moov).find(|(n, _)| n == b"mvex")?;
        let (_, mehd) = boites(mvex).find(|(n, _)| n == b"mehd")?;
        if *mehd.first()? == 1 { u64_a(mehd, 4)? } else { u32_a(mehd, 4)? }
    } else {
        duree
    };
    (echelle > 0 && duree > 0).then(|| duree as f64 / echelle as f64)
}
