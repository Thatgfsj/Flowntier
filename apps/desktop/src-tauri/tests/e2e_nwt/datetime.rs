//! datetime E2E tests.

use super::helpers::{civil_from_days, unix_secs_to_iso8601};

#[test]
fn e2e_simple_civil_from_days_known_dates() {
    assert_eq!(civil_from_days(0), (1970, 1, 1));
    assert_eq!(civil_from_days(1), (1970, 1, 2));
    // Reference dates (verified with Python: datetime.date(YYYY-MM-DD) - date(1970,1,1)):
    //   2000-01-01 = day 10957
    //   2024-02-29 = day 19782  (leap day)
    //   2026-06-27 = day 20631
    assert_eq!(civil_from_days(10957), (2000, 1, 1));
    assert_eq!(civil_from_days(19782), (2024, 2, 29));
    assert_eq!(civil_from_days(20631), (2026, 6, 27));
}

#[test]
fn e2e_simple_unix_secs_format() {
    let secs = 20631_i64 * 86_400 + 13 * 3600;
    let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs as u64);
    let s = unix_secs_to_iso8601(t);
    assert_eq!(s, "2026-06-27T13:00:00Z", "got: {s}");
}
