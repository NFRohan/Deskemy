//! Subtitle languages as download tools write them into file names: a code
//! (`en`, `eng`, `pt-BR`, `zh_Hans`) or the language's English name
//! (`English`, `Simplified Chinese`).

/// Two-letter code, the three-letter codes for it, and its English name.
const LANGUAGES: &[(&str, &[&str], &str)] = &[
    ("ar", &["ara"], "Arabic"),
    ("bn", &["ben"], "Bengali"),
    ("cs", &["ces", "cze"], "Czech"),
    ("da", &["dan"], "Danish"),
    ("de", &["deu", "ger"], "German"),
    ("el", &["ell", "gre"], "Greek"),
    ("en", &["eng"], "English"),
    ("es", &["spa"], "Spanish"),
    ("fa", &["fas", "per"], "Persian"),
    ("fi", &["fin"], "Finnish"),
    ("fil", &["fil", "tgl"], "Filipino"),
    ("fr", &["fra", "fre"], "French"),
    ("he", &["heb"], "Hebrew"),
    ("hi", &["hin"], "Hindi"),
    ("hu", &["hun"], "Hungarian"),
    ("id", &["ind"], "Indonesian"),
    ("it", &["ita"], "Italian"),
    ("ja", &["jpn"], "Japanese"),
    ("ko", &["kor"], "Korean"),
    ("ms", &["msa", "may"], "Malay"),
    ("nl", &["nld", "dut"], "Dutch"),
    ("no", &["nor", "nob"], "Norwegian"),
    ("pl", &["pol"], "Polish"),
    ("pt", &["por"], "Portuguese"),
    ("ro", &["ron", "rum"], "Romanian"),
    ("ru", &["rus"], "Russian"),
    ("sv", &["swe"], "Swedish"),
    ("ta", &["tam"], "Tamil"),
    ("te", &["tel"], "Telugu"),
    ("th", &["tha"], "Thai"),
    ("tr", &["tur"], "Turkish"),
    ("uk", &["ukr"], "Ukrainian"),
    ("ur", &["urd"], "Urdu"),
    ("vi", &["vie"], "Vietnamese"),
    ("zh", &["zho", "chi"], "Chinese"),
];

/// Names that aren't a plain language name: (name, code, label).
const VARIANTS: &[(&str, &str, &str)] = &[
    ("simplified chinese", "zh-hans", "Chinese (Simplified)"),
    ("traditional chinese", "zh-hant", "Chinese (Traditional)"),
    ("brazilian portuguese", "pt-br", "Portuguese (Brazil)"),
    ("portuguese (brazil)", "pt-br", "Portuguese (Brazil)"),
    ("latin american spanish", "es-419", "Spanish (Latin America)"),
];

/// The language a token names, as `(code, label)`: `"English"` →
/// `("en", "English")`, `"pt_BR"` → `("pt-br", "Portuguese (BR)")`,
/// `"RUS"` → `("ru", "Russian")`. `None` for anything else.
pub fn parse(token: &str) -> Option<(String, String)> {
    let lower = token.trim().to_lowercase();
    if let Some((_, code, label)) = VARIANTS.iter().find(|(name, _, _)| *name == lower) {
        return Some((code.to_string(), label.to_string()));
    }
    if let Some((code, _, name)) = LANGUAGES.iter().find(|(_, _, name)| name.eq_ignore_ascii_case(&lower)) {
        return Some((code.to_string(), name.to_string()));
    }
    let (base, region) = match lower.split_once(['-', '_']) {
        Some((base, region)) if (2..=4).contains(&region.len()) && region.chars().all(|c| c.is_ascii_alphanumeric()) => {
            (base, Some(region))
        }
        Some(_) => return None,
        None => (lower.as_str(), None),
    };
    let (code, _, name) = LANGUAGES.iter().find(|(code, three, _)| *code == base || three.contains(&base))?;
    Some(match region {
        Some(region) => (format!("{code}-{region}"), format!("{name} ({})", region.to_uppercase())),
        None => (code.to_string(), name.to_string()),
    })
}

/// A language spelled out, or a code too long to be a short word: what's
/// trusted at the end of a name that isn't known to be a subtitle's
/// (`What_it_is` doesn't end in Icelandic).
pub fn parse_unambiguous(token: &str) -> Option<(String, String)> {
    (token.trim().chars().count() >= 3).then(|| parse(token)).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(token: &str) -> Option<String> {
        parse(token).map(|(code, _)| code)
    }

    #[test]
    fn codes_names_and_regions() {
        assert_eq!(parse("en"), Some(("en".into(), "English".into())));
        assert_eq!(parse("English"), Some(("en".into(), "English".into())));
        assert_eq!(code("ENG"), Some("en".into()));
        assert_eq!(code("RUS"), Some("ru".into()));
        assert_eq!(parse("pt_BR"), Some(("pt-br".into(), "Portuguese (BR)".into())));
        assert_eq!(code("en-US"), Some("en-us".into()));
        assert_eq!(parse("Simplified Chinese"), Some(("zh-hans".into(), "Chinese (Simplified)".into())));
        assert_eq!(code("japanese"), Some("ja".into()));
    }

    #[test]
    fn not_a_language() {
        for token in ["", "Unix", "Part 2", "final", "en-", "en-toolong", "forced", "x"] {
            assert_eq!(parse(token), None, "{token}");
        }
    }

    #[test]
    fn short_codes_need_the_name_to_be_a_subtitle() {
        assert_eq!(parse_unambiguous("is"), None);
        assert_eq!(parse_unambiguous("it"), None);
        assert_eq!(parse_unambiguous("RUS").map(|(c, _)| c), Some("ru".into()));
        assert_eq!(parse_unambiguous("French").map(|(c, _)| c), Some("fr".into()));
    }
}
