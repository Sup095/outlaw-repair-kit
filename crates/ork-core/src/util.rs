//! Small shared helpers.

const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];

/// Render a byte count the way a person would say it.
pub fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    // One decimal below 10 keeps "9.4 GiB" readable without pretending to a
    // precision the underlying number does not have.
    if value < 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

/// Turn a stored timestamp into something worth reading.
///
/// Falls back to returning the input unchanged rather than to a placeholder:
/// an odd-looking timestamp is still evidence, and "unknown" is not.
pub fn readable_time(stored: &str) -> String {
    // Asking the operating system where it is can fail -- on Unix it is
    // refused outright in a multi-threaded process, which this is. Passed in
    // rather than looked up inside, so that both answers can be tested
    // without a test having to arrange for the machine to be somewhere.
    render_time(stored, time::UtcOffset::current_local_offset().ok())
}

/// The part of [`readable_time`] that does not depend on where the machine is.
///
/// `None` means the offset could not be established. That case is labelled
/// UTC rather than shown bare, because the one outcome worth ruling out is a
/// UTC time quietly presented as though it were local.
fn render_time(stored: &str, offset: Option<time::UtcOffset>) -> String {
    use time::format_description::well_known::Rfc3339;

    let Ok(instant) = time::OffsetDateTime::parse(stored, &Rfc3339) else {
        return stored.to_string();
    };

    let (moment, zone) = match offset {
        Some(offset) => (instant.to_offset(offset), String::new()),
        None => (instant.to_offset(time::UtcOffset::UTC), " UTC".to_string()),
    };

    let format = time::macros::format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");
    match moment.format(&format) {
        Ok(text) => format!("{text}{zone}"),
        Err(_) => stored.to_string(),
    }
}

/// A count and its noun, with the noun made plural when it needs to be.
///
/// Small, and worth having in one place: three screens were each writing
/// `item(s)` and `entries` rather than deciding, and "Last 1 entries" is the
/// sort of thing that makes a careful tool look careless on exactly the screen
/// where somebody is deciding whether to trust it.
///
/// English plurals are not this simple in general. These are the ones this
/// program actually counts -- items, entries, checks, problems, cores -- and a
/// caller with an irregular noun passes the plural in with [`counted_as`].
pub fn counted(count: usize, noun: &str) -> String {
    counted_as(count, noun, &format!("{noun}s"))
}

/// As [`counted`], for a noun whose plural is not the singular plus `s`.
pub fn counted_as(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        format!("{count} {one}")
    } else {
        format!("{count} {many}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_across_units() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.0 KiB");
        assert_eq!(format_bytes(1536), "1.5 KiB");
        assert_eq!(format_bytes(15 * 1024 * 1024), "15 MiB");
        assert_eq!(format_bytes(2 * 1024 * 1024 * 1024), "2.0 GiB");
    }

    #[test]
    fn a_stored_timestamp_is_rewritten_for_a_person_to_read() {
        // Seven digits of fractional second and a `T` in the middle is what
        // the database should hold and is not what anybody should be shown.
        let readable = readable_time("2026-08-25T23:43:30.4183399Z");
        assert!(!readable.contains('T'), "{readable}");
        assert!(!readable.contains(".41"), "{readable}");
        assert!(readable.starts_with("2026-08-2"), "{readable}");
        assert!(readable.contains(':'), "{readable}");
    }

    #[test]
    fn a_time_that_could_not_be_localised_says_utc() {
        // The outcome worth ruling out is a UTC time presented as though it
        // were local, so when the offset is unknown it must be labelled.
        //
        // The offset is passed in rather than looked up: an earlier version of
        // this test read whatever the machine happened to be set to, passed
        // here, and failed on both CI runners -- which sit in UTC, where a
        // correctly localised time is indistinguishable from an unlocalised
        // one. That test was measuring the runner, not the code.
        let utc = render_time("2026-08-25T23:43:30Z", None);
        assert_eq!(utc, "2026-08-25 23:43:30 UTC");
    }

    #[test]
    fn a_localised_time_is_moved_and_left_unlabelled() {
        let five_west = time::UtcOffset::from_hms(-5, 0, 0).unwrap();
        let local = render_time("2026-08-25T23:43:30Z", Some(five_west));
        assert_eq!(local, "2026-08-25 18:43:30");
    }

    #[test]
    fn a_machine_that_really_is_on_utc_is_not_labelled() {
        // Zero offset is a location, not a failure to find one. Labelling it
        // would be telling somebody in London that their clock is foreign.
        let here = render_time("2026-08-25T23:43:30Z", Some(time::UtcOffset::UTC));
        assert_eq!(here, "2026-08-25 23:43:30");
    }

    #[test]
    fn something_that_is_not_a_timestamp_survives_unchanged() {
        // An odd-looking timestamp is still evidence. Replacing it with
        // "unknown" would throw away the only clue about what went wrong.
        assert_eq!(readable_time("not a date"), "not a date");
        assert_eq!(readable_time(""), "");
    }

    #[test]
    fn one_of_something_is_singular_and_everything_else_is_not() {
        assert_eq!(counted(1, "entry_x"), "1 entry_x");
        assert_eq!(counted(0, "item"), "0 items");
        assert_eq!(counted(2, "item"), "2 items");
        assert_eq!(counted(1, "item"), "1 item");
    }

    #[test]
    fn an_irregular_plural_is_given_rather_than_guessed() {
        assert_eq!(counted_as(1, "entry", "entries"), "1 entry");
        assert_eq!(counted_as(3, "entry", "entries"), "3 entries");
        // Zero takes the plural in English, which is the case a naive
        // `if count > 1` gets wrong.
        assert_eq!(counted_as(0, "entry", "entries"), "0 entries");
    }
}
