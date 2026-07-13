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
}
