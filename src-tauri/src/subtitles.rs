//! Minimal SRT/VTT subtitle parser — just enough to extract (start_ms, text)
//! cues for the full-text subtitle index.

/// Parse subtitle file contents into `(start_ms, text)` cues. Handles both SRT
/// (`00:00:01,000`) and VTT (`00:00:01.000`) timing, joins multi-line cue text,
/// and strips inline tags.
pub fn parse(content: &str) -> Vec<(i64, String)> {
    let mut cues = Vec::new();
    let mut lines = content.lines().peekable();
    while let Some(line) = lines.next() {
        let Some(start) = parse_start(line) else {
            continue;
        };
        let mut text_lines = Vec::new();
        while let Some(next) = lines.peek() {
            if next.trim().is_empty() {
                break;
            }
            text_lines.push(lines.next().unwrap());
        }
        let text = clean(&text_lines.join(" "));
        if !text.is_empty() {
            cues.push((start, text));
        }
    }
    cues
}

/// If `line` is a cue-timing line (`start --> end [settings]`), return start ms.
fn parse_start(line: &str) -> Option<i64> {
    let (left, _rest) = line.split_once("-->")?;
    parse_ts(left.trim())
}

/// Parse `HH:MM:SS,mmm` / `HH:MM:SS.mmm` / `MM:SS.mmm` into milliseconds.
fn parse_ts(s: &str) -> Option<i64> {
    let s = s.replace(',', ".");
    let (hms, frac) = s.split_once('.').unwrap_or((s.as_str(), "0"));
    let mut parts = hms.split(':').rev();
    let sec: i64 = parts.next()?.trim().parse().ok()?;
    let min: i64 = parts.next().unwrap_or("0").trim().parse().ok()?;
    let hour: i64 = parts.next().unwrap_or("0").trim().parse().ok()?;
    let digits: String = frac.chars().take(3).collect();
    let millis: i64 = format!("{digits:0<3}").parse().unwrap_or(0);
    Some((hour * 3600 + min * 60 + sec) * 1000 + millis)
}

/// Strip `<...>` tags and collapse whitespace.
fn clean(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_srt() {
        let s = "1\n00:00:01,500 --> 00:00:03,000\nHello <i>world</i>\n\n2\n00:01:02,250 --> 00:01:04,000\nSecond line\n";
        let cues = parse(s);
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0], (1500, "Hello world".to_string()));
        assert_eq!(cues[1].0, 62250);
    }

    #[test]
    fn parses_vtt() {
        let s = "WEBVTT\n\n00:00:05.000 --> 00:00:06.000 align:start\nA cue\n";
        let cues = parse(s);
        assert_eq!(cues, vec![(5000, "A cue".to_string())]);
    }
}
