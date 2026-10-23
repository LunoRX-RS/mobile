use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::America::Los_Angeles;

/// Days since 1970-01-01 of the Pacific-local calendar date of `now_utc`; 0 before the epoch.
pub fn pacific_day(now_utc: DateTime<Utc>) -> u64 {
    let epoch = NaiveDate::from_ymd_opt(1970, 1, 1).expect("valid epoch");
    let days = now_utc
        .with_timezone(&Los_Angeles)
        .date_naive()
        .signed_duration_since(epoch)
        .num_days();
    u64::try_from(days).unwrap_or(0)
}

pub fn today() -> u64 {
    pacific_day(Utc::now())
}

#[cfg(test)]
mod tests {
    use super::pacific_day;
    use chrono::{DateTime, NaiveDate, Utc};

    // Day number of a calendar date, as pacific_day returns it: days since 1970-01-01.
    fn day_of(y: i32, m: u32, d: u32) -> u64 {
        let epoch = NaiveDate::from_ymd_opt(1970, 1, 1).unwrap();
        let date = NaiveDate::from_ymd_opt(y, m, d).unwrap();
        date.signed_duration_since(epoch).num_days() as u64
    }

    fn utc(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    #[test]
    fn pst_just_before_spring_forward_is_same_day() {
        // 09:59:59Z = 01:59:59 PST on 2026-03-08 (DST starts at 10:00Z).
        assert_eq!(pacific_day(utc("2026-03-08T09:59:59Z")), day_of(2026, 3, 8));
    }

    #[test]
    fn pdt_just_after_spring_forward_is_same_day() {
        // 10:00:00Z = 03:00 PDT on 2026-03-08.
        assert_eq!(pacific_day(utc("2026-03-08T10:00:00Z")), day_of(2026, 3, 8));
    }

    #[test]
    fn last_second_of_pacific_day_is_previous_utc_day() {
        // 06:59:59Z = 23:59:59 PDT on 2026-10-17 (UTC is already 2026-10-18).
        assert_eq!(
            pacific_day(utc("2026-10-18T06:59:59Z")),
            day_of(2026, 10, 17)
        );
    }

    #[test]
    fn pacific_midnight_starts_new_day() {
        // 07:00:00Z = 00:00 PDT on 2026-10-18.
        assert_eq!(
            pacific_day(utc("2026-10-18T07:00:00Z")),
            day_of(2026, 10, 18)
        );
    }

    #[test]
    fn before_epoch_returns_zero() {
        // 1969-12-31T23:59:59Z: negative day count clamps to 0, no panic.
        assert_eq!(pacific_day(utc("1969-12-31T23:59:59Z")), 0);
    }
}
