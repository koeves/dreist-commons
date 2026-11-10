//! Civil time: local through the C library, UTC by arithmetic.

/// Year, month, day, hour, minute, second.
pub type Civil = (i64, u32, u32, u32, u32, u32);

fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Local civil time now, as the C library's `strftime` formats it; UTC
/// when the C library cannot tell.
pub fn now_local() -> Civil {
    let secs = now_secs();
    let t = secs as libc::time_t;
    // SAFETY: `tm` is plain data; the C call fills it from `t`.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    #[cfg(unix)]
    let ok = !unsafe { libc::localtime_r(&t, &mut tm) }.is_null();
    #[cfg(windows)]
    let ok = unsafe { libc::localtime_s(&mut tm, &t) } == 0;
    #[cfg(not(any(unix, windows)))]
    let ok = false;
    if ok {
        return (tm.tm_year as i64 + 1900, tm.tm_mon as u32 + 1, tm.tm_mday as u32, tm.tm_hour as u32, tm.tm_min as u32, tm.tm_sec as u32);
    }
    civil_utc(secs)
}

/// UTC civil time now.
pub fn now_utc() -> Civil {
    civil_utc(now_secs())
}

/// UTC civil time of a Unix time.
pub fn civil_utc(secs: i64) -> Civil {
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + (m <= 2) as i64;
    (y, m, d, (rem / 3600) as u32, (rem / 60 % 60) as u32, (rem % 60) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_times_to_civil_utc() {
        assert_eq!(civil_utc(0), (1970, 1, 1, 0, 0, 0));
        assert_eq!(civil_utc(1_452_171_504), (2016, 1, 7, 12, 58, 24));
        assert_eq!(civil_utc(-1), (1969, 12, 31, 23, 59, 59));
    }

    #[test]
    fn local_time_is_a_valid_date() {
        let (y, mo, d, h, mi, s) = now_local();
        assert!(y >= 2024 && (1..=12).contains(&mo) && (1..=31).contains(&d) && h < 24 && mi < 60 && s < 61);
    }
}
