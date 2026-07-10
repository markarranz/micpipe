use std::{
    fmt,
    time::{SystemTime, UNIX_EPOCH},
};

pub fn stdout(args: fmt::Arguments<'_>) {
    println!("[{}] {}", timestamp_local(), args);
}

pub fn stderr(args: fmt::Arguments<'_>) {
    eprintln!("[{}] {}", timestamp_local(), args);
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

fn timestamp_local() -> String {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let unix_seconds = duration.as_secs();

    timestamp_from_unix_local(unix_seconds).unwrap_or_else(|| timestamp_from_unix_utc(unix_seconds))
}

#[cfg(unix)]
fn timestamp_from_unix_local(unix_seconds: u64) -> Option<String> {
    use std::mem::MaybeUninit;

    let raw_time: libc::time_t = unix_seconds.try_into().ok()?;
    let mut local_time = MaybeUninit::<libc::tm>::uninit();

    // SAFETY: `local_time` points to valid, writable memory and `raw_time` lives long
    // enough for the call. `localtime_r` initializes `local_time` when it succeeds.
    let local_time = unsafe {
        if libc::localtime_r(&raw const raw_time, local_time.as_mut_ptr()).is_null() {
            return None;
        }
        local_time.assume_init()
    };

    let year = i64::from(local_time.tm_year) + 1_900;
    let month = i64::from(local_time.tm_mon) + 1;
    let day = i64::from(local_time.tm_mday);
    let hour = i64::from(local_time.tm_hour);
    let minute = i64::from(local_time.tm_min);
    let second = i64::from(local_time.tm_sec);
    let local_seconds =
        days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second;
    let offset_seconds = local_seconds.checked_sub(unix_seconds.try_into().ok()?)?;

    Some(format_timestamp(
        year,
        month,
        day,
        hour,
        minute,
        second,
        offset_seconds,
    ))
}

#[cfg(not(unix))]
fn timestamp_from_unix_local(_unix_seconds: u64) -> Option<String> {
    None
}

fn timestamp_from_unix_utc(unix_seconds: u64) -> String {
    let days = (unix_seconds / 86_400) as i64;
    let seconds_of_day = unix_seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;

    format_timestamp(
        year,
        month,
        day,
        hour as i64,
        minute as i64,
        second as i64,
        0,
    )
}

fn format_timestamp(
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
    offset_seconds: i64,
) -> String {
    let offset_sign = if offset_seconds < 0 { '-' } else { '+' };
    let offset_abs = offset_seconds.abs();
    let offset_hours = offset_abs / 3_600;
    let offset_minutes = (offset_abs % 3_600) / 60;

    format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}{offset_sign}{offset_hours:02}:{offset_minutes:02}"
    )
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_prime = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_prime + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;

    era * 146_097 + day_of_era - 719_468
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
    let year = year + i64::from(month <= 2);

    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::{format_timestamp, timestamp_from_unix_utc};

    #[test]
    fn formats_unix_epoch_as_utc_fallback() {
        assert_eq!(timestamp_from_unix_utc(0), "1970-01-01T00:00:00+00:00");
    }

    #[test]
    fn formats_leap_day_as_utc_fallback() {
        assert_eq!(
            timestamp_from_unix_utc(1_709_251_199),
            "2024-02-29T23:59:59+00:00"
        );
    }

    #[test]
    fn formats_local_offset() {
        assert_eq!(
            format_timestamp(2026, 6, 29, 13, 4, 5, -7 * 3_600),
            "2026-06-29T13:04:05-07:00"
        );
    }
}
