use std::{
    fmt,
    time::{SystemTime, UNIX_EPOCH},
};

pub fn stdout(args: fmt::Arguments<'_>) {
    println!("[{}] {}", timestamp_utc(), args);
}

pub fn stderr(args: fmt::Arguments<'_>) {
    eprintln!("[{}] {}", timestamp_utc(), args);
}

#[macro_export]
macro_rules! log_out {
    ($($arg:tt)*) => {
        $crate::logging::stdout(format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! log_err {
    ($($arg:tt)*) => {
        $crate::logging::stderr(format_args!($($arg)*))
    };
}

fn timestamp_utc() -> String {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    timestamp_from_unix(duration.as_secs())
}

fn timestamp_from_unix(unix_seconds: u64) -> String {
    let days = (unix_seconds / 86_400) as i64;
    let seconds_of_day = unix_seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let days = days_since_epoch + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    let year = year + if month <= 2 { 1 } else { 0 };

    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::timestamp_from_unix;

    #[test]
    fn formats_unix_epoch() {
        assert_eq!(timestamp_from_unix(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn formats_leap_day() {
        assert_eq!(timestamp_from_unix(1_709_251_199), "2024-02-29T23:59:59Z");
    }
}
