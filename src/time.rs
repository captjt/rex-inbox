//! Timestamps: stored as UTC RFC 3339, shown as ages.

use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

/// Now, to the second, so stored times compare as strings with SQLite's.
pub fn now() -> String {
    fmt(OffsetDateTime::now_utc())
}

fn fmt(t: OffsetDateTime) -> String {
    t.replace_nanosecond(0).expect("zero is valid").format(&Rfc3339).expect("format time")
}

pub fn parse(s: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(s, &Rfc3339).ok()
}

pub fn after(d: Duration) -> String {
    fmt(OffsetDateTime::now_utc() + d)
}

/// `45s`, `3m`, `2h`, `4d`.
pub fn age(since: &str) -> String {
    let Some(t) = parse(since) else { return "?".into() };
    let secs = (OffsetDateTime::now_utc() - t).whole_seconds().max(0);
    match secs {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m", s / 60),
        s if s < 86400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86400),
    }
}

/// Parse `90s`, `15m`, `1h`, `2d`.
pub fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    let (num, unit) = s.split_at(s.find(|c: char| !c.is_ascii_digit())?);
    let n: i64 = num.parse().ok()?;
    Some(match unit {
        "s" => Duration::seconds(n),
        "m" => Duration::minutes(n),
        "h" => Duration::hours(n),
        "d" => Duration::days(n),
        _ => return None,
    })
}
