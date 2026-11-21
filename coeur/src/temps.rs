//! Dates et heures des réunions, sans fuseau : l'heure locale telle que la donne l'horloge.

/// Un instant à la minute près, en heure locale.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DateHeure {
    pub annee: i32,
    pub mois: u8,
    pub jour: u8,
    pub heure: u8,
    pub minute: u8,
}

const JOURS: [&str; 7] = ["lun.", "mar.", "mer.", "jeu.", "ven.", "sam.", "dim."];

impl DateHeure {
    pub const fn new(annee: i32, mois: u8, jour: u8, heure: u8, minute: u8) -> DateHeure {
        DateHeure { annee, mois, jour, heure, minute }
    }

    /// Jours depuis le 1er janvier 1970 (algorithme « days from civil »).
    fn jours(&self) -> i64 {
        let m = i64::from(self.mois);
        let y = i64::from(self.annee) - i64::from(m <= 2);
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400;
        let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(self.jour) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    /// L'instant `secondes` après le 1er janvier 1970 à minuit, en temps universel (algorithme
    /// « civil from days »).
    pub fn depuis_unix(secondes: i64) -> DateHeure {
        let (jours, s) = (secondes.div_euclid(86_400), secondes.rem_euclid(86_400));
        let z = jours + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let jour = (doy - (153 * mp + 2) / 5 + 1) as u8;
        let mois = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
        let annee = (yoe + era * 400 + i64::from(mois <= 2)) as i32;
        DateHeure::new(annee, mois, jour, (s / 3600) as u8, ((s / 60) % 60) as u8)
    }

    /// Nombre de jours du calendrier entre cette date et `plus_tard` (0 le même jour).
    pub fn jours_jusqu_a(&self, plus_tard: DateHeure) -> i64 {
        plus_tard.jours() - self.jours()
    }

    /// 0 pour lundi, 6 pour dimanche.
    pub fn jour_de_la_semaine(&self) -> usize {
        (self.jours() + 3).rem_euclid(7) as usize
    }

    /// « sam. 21/11 »
    pub fn jour_court(&self) -> String {
        format!("{} {:02}/{:02}", JOURS[self.jour_de_la_semaine()], self.jour, self.mois)
    }

    /// « 14h02 »
    pub fn heure_courte(&self) -> String {
        format!("{:02}h{:02}", self.heure, self.minute)
    }

    /// « 2026-11-21 14h02 », le début du nom du dossier d'une réunion.
    pub fn horodatage(&self) -> String {
        format!("{:04}-{:02}-{:02} {}", self.annee, self.mois, self.jour, self.heure_courte())
    }
}

/// Secondes depuis le début de la réunion.
pub fn hms(h: u32, m: u32, s: u32) -> f64 {
    f64::from(h * 3600 + m * 60 + s)
}

/// « 0:44:12 », la forme des horodatages du bilan et de la transcription.
pub fn horodatage_de_lecture(t: f64) -> String {
    let t = t.max(0.0) as u64;
    format!("{}:{:02}:{:02}", t / 3600, (t / 60) % 60, t % 60)
}
