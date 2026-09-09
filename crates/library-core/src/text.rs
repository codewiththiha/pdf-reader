//! The numbers the library shows as words: how big a file is, how long ago
//! something happened, and how many of a thing there are.
//!
//! All three live here rather than in a view because all three are rules
//! rather than presentation — "12.4 MB", "3 days ago" and "3 books" are
//! answers a test can hold to account, and a restore row that said "12 MB"
//! while the modal said "12.4 MB" would be two components disagreeing about
//! one file.

/// A byte count as a reader would say it.
///
/// Binary units, decimal spelling: `1024 * 1024` bytes reads as "1.0 MB" and not
/// as "1.0 MiB", because a shelf of PDFs is not a place anybody wants a
/// standards argument. One decimal below 10 of a unit and none above it, so the
/// string is the same width whatever it is describing — "840 KB", "3.2 MB",
/// "14 GB" — which is what lets a menu row and a receipt line share a column.
pub fn human_size(bytes: u64) -> String {
    const STEP: f64 = 1024.0;
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= STEP && unit < UNITS.len() - 1 {
        value /= STEP;
        unit += 1;
    }
    if unit == 0 {
        // Bytes are whole or they are not bytes.
        return format!("{bytes} B");
    }
    if value < 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{:.0} {}", value, UNITS[unit])
    }
}

/// How long ago `then_ms` was, as a reader would say it.
///
/// Deliberately coarse below a minute and above a week: "3 minutes ago" is a
/// useful thing to know about a book you just removed and "4 months ago" is not,
/// and a menu that counted hours past the second day would be a clock where a
/// sentence belongs. A stamp in the future (a clock that moved, a hand-edited
/// blob) reads as "just now" rather than as a negative age.
pub fn human_age(then_ms: u64, now_ms: u64) -> String {
    const MINUTE: u64 = 60_000;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;
    const WEEK: u64 = 7 * DAY;
    let elapsed = now_ms.saturating_sub(then_ms);
    if elapsed < MINUTE {
        return "just now".to_string();
    }
    if elapsed < HOUR {
        return ago(elapsed / MINUTE, "minute", "minutes");
    }
    if elapsed < DAY {
        return ago(elapsed / HOUR, "hour", "hours");
    }
    if elapsed < WEEK {
        return ago(elapsed / DAY, "day", "days");
    }
    ago(elapsed / WEEK, "week", "weeks")
}

fn ago(count: u64, one: &str, many: &str) -> String {
    format!("{} ago", plural(count as usize, one, many))
}

/// A count as a reader would say it: "1 book", "3 books".
///
/// One rule rather than a `match` per sentence, because the shelf says this in
/// a folder's summary line, in the removal receipt, in the import dock's
/// headline and in the search bar's promise — and the singular of "shelves" is
/// easy to get wrong once and never notice.
pub fn plural(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{count} {many}")
    }
}

#[cfg(test)]
mod tests {
    use super::{human_age, human_size, plural};

    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    #[test]
    fn bytes_read_as_bytes() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(1), "1 B");
        assert_eq!(human_size(1023), "1023 B");
    }

    #[test]
    fn a_size_keeps_one_decimal_until_it_does_not_need_one() {
        assert_eq!(human_size(KB), "1.0 KB");
        assert_eq!(human_size(3 * MB + 200 * KB), "3.2 MB");
        assert_eq!(human_size(3 * GB + 200 * MB), "3.2 GB");
        // Past ten of a unit the decimal is noise, and dropping it is what keeps
        // every size the same width in a menu column.
        assert_eq!(human_size(12 * MB + 400 * KB), "12 MB");
        assert_eq!(human_size(30 * KB), "30 KB");
        assert_eq!(human_size(500 * MB), "500 MB");
        assert_eq!(human_size(14 * GB), "14 GB");
    }

    #[test]
    fn the_units_stop_at_the_last_one_they_have() {
        // A shelf of books does not reach a petabyte, and a row that said
        // "1024 TB" would be a bug with an extra step in it.
        assert_eq!(human_size(2048 * GB), "2.0 TB");
        assert!(
            human_size(u64::MAX).ends_with(" TB"),
            "the largest count a u64 can hold still gets a unit"
        );
    }

    #[test]
    fn an_age_is_coarse_where_coarse_is_enough() {
        let now = 1_700_000_000_000u64;
        let minute = 60_000;
        assert_eq!(human_age(now, now), "just now");
        assert_eq!(human_age(now - 30_000, now), "just now");
        assert_eq!(human_age(now - minute, now), "1 minute ago");
        assert_eq!(human_age(now - 3 * minute, now), "3 minutes ago");
        assert_eq!(human_age(now - 59 * minute, now), "59 minutes ago");
        assert_eq!(human_age(now - 60 * minute, now), "1 hour ago");
        assert_eq!(human_age(now - 5 * 60 * minute, now), "5 hours ago");
        assert_eq!(human_age(now - 24 * 60 * minute, now), "1 day ago");
        assert_eq!(human_age(now - 3 * 24 * 60 * minute, now), "3 days ago");
        assert_eq!(human_age(now - 6 * 24 * 60 * minute, now), "6 days ago");
        assert_eq!(human_age(now - 7 * 24 * 60 * minute, now), "1 week ago");
        assert_eq!(human_age(now - 21 * 24 * 60 * minute, now), "3 weeks ago");
    }

    #[test]
    fn a_count_reads_as_one_thing_or_many() {
        assert_eq!(plural(1, "book", "books"), "1 book");
        assert_eq!(plural(3, "book", "books"), "3 books");
        assert_eq!(plural(0, "shelf", "shelves"), "0 shelves");
        // The irregular plural is the caller's to spell, which is the whole of
        // why the rule takes both words.
        assert_eq!(plural(2, "shelf", "shelves"), "2 shelves");
    }

    #[test]
    fn a_stamp_in_the_future_is_not_a_negative_age() {
        // A clock that moved, or a blob written by a machine ahead of this one,
        // must not produce "minus 4 hours ago" on a restore row.
        assert_eq!(human_age(1_800_000_000_000, 1_700_000_000_000), "just now");
    }
}
