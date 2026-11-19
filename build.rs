//! Links the static FFmpeg (`ffmpeg-link`, shared with the crates that
//! link it too) and dates the build, for About.

use std::env;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    date_the_build();
    ffmpeg_link::link();
}

/// `PLAYS_BUILD_DATE` (YYYY-MM-DD, UTC): the day of `SOURCE_DATE_EPOCH` for
/// a reproducible build, else today. Dated again whenever the source changes.
fn date_the_build() {
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");
    let seconds = env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|epoch| epoch.parse::<i64>().ok())
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |since| since.as_secs() as i64)
        });
    let (year, month, day) = civil_date(seconds.div_euclid(86_400));
    println!("cargo:rustc-env=PLAYS_BUILD_DATE={year:04}-{month:02}-{day:02}");
}

/// The date `days` after 1970-01-01, in the proleptic Gregorian calendar
/// (Howard Hinnant's `civil_from_days`).
fn civil_date(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u32;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}
