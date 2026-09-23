//! Pure helpers for turning raw file/folder names into ordered, cleaned titles.

/// First integer appearing in a name (`"01 - Intro"` → 1, `"Section 3"` → 3).
/// `None` when there are no digits.
pub fn leading_number(name: &str) -> Option<i64> {
    let bytes = name.as_bytes();
    let mut i = 0;
    while i < bytes.len() && !bytes[i].is_ascii_digit() {
        i += 1;
    }
    let start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if start == i {
        None
    } else {
        name[start..i].parse::<i64>().ok()
    }
}

/// Display title: drop the extension, then strip a leading numeric prefix and
/// its separator (`"001 Welcome.mp4"` → `"Welcome"`, `"1 - Intro"` → `"Intro"`).
/// Leaves numeric-only titles like `"1984"` intact (no separator follows).
pub fn clean_title(file_name: &str) -> String {
    let stem = match file_name.rfind('.') {
        Some(idx) if idx > 0 => &file_name[..idx],
        _ => file_name,
    };

    let bytes = stem.as_bytes();
    let mut i = 0;
    while i < bytes.len() && (bytes[i] as char).is_whitespace() {
        i += 1;
    }
    let digits_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }

    if i > digits_start {
        let mut k = i;
        while k < bytes.len()
            && matches!(
                bytes[k] as char,
                ' ' | '.' | '-' | '_' | ')' | ']' | ':' | '\t' | '|'
            )
        {
            k += 1;
        }
        // Only strip when a separator actually followed the digits.
        if k > i {
            return stem[k..].trim().to_string();
        }
    }

    stem.trim().to_string()
}

/// Sort key: numbered items first (ascending), unnumbered after, alphabetical
/// as the tiebreak. Use with `sort_by_key`.
pub fn sort_key(name: &str) -> (bool, i64, String) {
    let n = leading_number(name);
    (n.is_none(), n.unwrap_or(0), name.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_leading_numbers() {
        assert_eq!(leading_number("01 - Intro"), Some(1));
        assert_eq!(leading_number("1. Welcome"), Some(1));
        assert_eq!(leading_number("Section 3"), Some(3));
        assert_eq!(leading_number("10 Things.mp4"), Some(10));
        assert_eq!(leading_number("Introduction"), None);
    }

    #[test]
    fn cleans_titles() {
        assert_eq!(clean_title("001 Welcome.mp4"), "Welcome");
        assert_eq!(clean_title("1 - Getting Started.mp4"), "Getting Started");
        assert_eq!(clean_title("01_setup_env.mp4"), "setup_env");
        assert_eq!(clean_title("Introduction.mp4"), "Introduction");
        assert_eq!(clean_title("1984.mp4"), "1984");
    }

    #[test]
    fn orders_naturally() {
        let mut v = vec!["10 b", "2 a", "1 a", "Bonus"];
        v.sort_by_key(|s| sort_key(s));
        assert_eq!(v, vec!["1 a", "2 a", "10 b", "Bonus"]);
    }

    // ── Property tests: invariants that must hold for arbitrary input ──
    use proptest::prelude::*;

    proptest! {
        /// Never panics (esp. on multi-byte UTF-8), and the result is trimmed.
        #[test]
        fn clean_title_is_total_and_trimmed(s in ".*") {
            let t = clean_title(&s);
            prop_assert_eq!(t.trim(), &t);
        }

        /// leading_number never panics and, when Some, that run of digits is
        /// actually present in the string.
        #[test]
        fn leading_number_is_present(s in ".*") {
            if let Some(n) = leading_number(&s) {
                prop_assert!(s.contains(&n.to_string()));
            }
        }

        /// A pure "<n>" string parses back to exactly n.
        #[test]
        fn leading_number_roundtrips(n in 0i64..1_000_000_000) {
            prop_assert_eq!(leading_number(&n.to_string()), Some(n));
        }

        /// "<n> <word>.mp4" (word alphabetic) cleans to just the word.
        #[test]
        fn clean_title_strips_numeric_prefix(n in 0u32..100_000, word in "[A-Za-z][A-Za-z]{0,20}") {
            let name = format!("{n} {word}.mp4");
            prop_assert_eq!(clean_title(&name), word);
        }

        /// After sorting by `sort_key`: every numbered item precedes every
        /// unnumbered one, and numbered items are in non-decreasing order.
        #[test]
        fn sort_key_numbered_first_then_ascending(
            names in prop::collection::vec("(0|[1-9][0-9]{0,4})? ?[a-z]{1,6}", 0..15)
        ) {
            let mut v = names.clone();
            v.sort_by_key(|s| sort_key(s));
            let nums: Vec<Option<i64>> = v.iter().map(|s| leading_number(s)).collect();
            // No numbered item appears after the first unnumbered one.
            if let Some(first_none) = nums.iter().position(Option::is_none) {
                prop_assert!(nums[first_none..].iter().all(Option::is_none));
            }
            // Numbered items are non-decreasing.
            let numbered: Vec<i64> = nums.iter().flatten().copied().collect();
            prop_assert!(numbered.windows(2).all(|w| w[0] <= w[1]));
        }
    }
}
