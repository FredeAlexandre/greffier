//! Meeting dates and times, without a time zone: local time as the clock gives it.

/// An instant to the nearest minute, in local time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DateTime {
    pub year: i32,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
}

const DAYS: [&str; 7] = ["lun.", "mar.", "mer.", "jeu.", "ven.", "sam.", "dim."];

impl DateTime {
    pub const fn new(year: i32, month: u8, day: u8, hour: u8, minute: u8) -> DateTime {
        DateTime { year, month, day, hour, minute }
    }

    /// Days since January 1, 1970 (the "days from civil" algorithm).
    fn days(&self) -> i64 {
        let m = i64::from(self.month);
        let y = i64::from(self.year) - i64::from(m <= 2);
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400;
        let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(self.day) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    /// The instant `seconds` after January 1, 1970 at midnight, in universal time (the
    /// "civil from days" algorithm).
    pub fn from_unix(seconds: i64) -> DateTime {
        let (days, s) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
        let year = (yoe + era * 400 + i64::from(month <= 2)) as i32;
        DateTime::new(year, month, day, (s / 3600) as u8, ((s / 60) % 60) as u8)
    }

    /// Number of calendar days between this date and `later` (0 on the same day).
    pub fn days_until(&self, later: DateTime) -> i64 {
        later.days() - self.days()
    }

    /// 0 for Monday, 6 for Sunday.
    pub fn weekday(&self) -> usize {
        (self.days() + 3).rem_euclid(7) as usize
    }

    /// "sam. 21/11"
    pub fn short_day(&self) -> String {
        format!("{} {:02}/{:02}", DAYS[self.weekday()], self.day, self.month)
    }

    /// "samedi 21 novembre 2026"
    pub fn long_day(&self) -> String {
        const WEEKDAYS: [&str; 7] = ["lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche"];
        const MONTHS: [&str; 12] = ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"];
        let month = MONTHS.get(usize::from(self.month).wrapping_sub(1)).copied().unwrap_or("?");
        format!("{} {} {month} {}", WEEKDAYS[self.weekday()], self.day, self.year)
    }

    /// "14h02"
    pub fn short_time(&self) -> String {
        format!("{:02}h{:02}", self.hour, self.minute)
    }

    /// "2026-11-21 14h02", the start of a meeting's folder name.
    pub fn timestamp(&self) -> String {
        format!("{:04}-{:02}-{:02} {}", self.year, self.month, self.day, self.short_time())
    }
}

/// Seconds since the start of the meeting.
pub fn hms(h: u32, m: u32, s: u32) -> f64 {
    f64::from(h * 3600 + m * 60 + s)
}

/// "0:44:12", the form of the summary's and transcription's timestamps.
pub fn playback_timestamp(t: f64) -> String {
    let t = t.max(0.0) as u64;
    format!("{}:{:02}:{:02}", t / 3600, (t / 60) % 60, t % 60)
}
