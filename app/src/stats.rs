//! The playback-info overlay (I), Plezy-style: what mpv is decoding and how
//! well it keeps up. Only sampled while the overlay is open.

use crate::{StatGroup, StatLine, StatRow};
use deskemy_core::mpv::Mpv;

/// Stats as plain data, so they can be built on the event thread.
pub type Groups = Vec<(&'static str, Vec<(&'static str, String)>)>;

pub fn read(mpv: &Mpv) -> Groups {
    let text = |name: &str| mpv.get_property_string(name);
    let num = |name: &str| mpv.get_f64(name);
    let or_dash = |v: Option<String>| v.unwrap_or_else(|| "—".into());

    let resolution = match (num("video-params/w"), num("video-params/h")) {
        (Some(w), Some(h)) => Some(format!("{w}x{h}")),
        _ => None,
    };
    let dropped = [num("frame-drop-count"), num("decoder-frame-drop-count")]
        .into_iter()
        .flatten()
        .sum::<f64>();

    vec![
        (
            "Video",
            vec![
                ("Codec", or_dash(text("current-tracks/video/codec").map(|c| codec_name(&c)))),
                ("Resolution", or_dash(resolution)),
                ("FPS", or_dash(num("container-fps").map(|f| format!("{f:.2}")))),
                ("Bitrate", or_dash(num("video-bitrate").map(bitrate))),
                ("Decoder", text("hwdec-current").unwrap_or_else(|| "software".into())),
            ],
        ),
        (
            "Audio",
            vec![
                ("Codec", or_dash(text("current-tracks/audio/codec").map(|c| codec_name(&c)))),
                ("Sample rate", or_dash(num("audio-params/samplerate").map(|r| format!("{:.1} kHz", r / 1000.0)))),
                ("Channels", or_dash(text("audio-params/channel-count"))),
                ("Bitrate", or_dash(num("audio-bitrate").map(bitrate))),
            ],
        ),
        (
            "Color",
            vec![
                ("Pixel format", or_dash(text("video-params/pixelformat"))),
                ("HW format", or_dash(text("video-params/hw-pixelformat"))),
                ("Matrix", or_dash(text("video-params/colormatrix"))),
                ("Primaries", or_dash(text("video-params/primaries"))),
                ("Transfer", or_dash(text("video-params/gamma"))),
            ],
        ),
        (
            "Performance",
            vec![
                ("Render FPS", or_dash(num("estimated-vf-fps").map(|f| format!("{f:.2}")))),
                ("A/V sync", or_dash(num("avsync").map(milliseconds))),
                ("Dropped", format!("{dropped}")),
            ],
        ),
        (
            "Buffer",
            vec![
                ("Cached", or_dash(num("demuxer-cache-duration").map(|s| format!("{s:.1}s")))),
                ("Speed", or_dash(num("cache-speed").map(|b| format!("{:.1} MB/s", b / 1_000_000.0)))),
            ],
        ),
        (
            "App",
            vec![
                ("Player", or_dash(text("mpv-version"))),
                ("Renderer", "Slint · OpenGL".into()),
            ],
        ),
    ]
}

/// Lay the groups out in rows of three, as the overlay shows them.
pub fn rows(groups: Groups) -> Vec<StatRow> {
    let groups: Vec<StatGroup> = groups
        .into_iter()
        .map(|(title, lines)| StatGroup {
            title: title.into(),
            lines: crate::course_panel::model(
                lines
                    .into_iter()
                    .map(|(label, value)| StatLine { label: label.into(), value: value.into() })
                    .collect(),
            ),
        })
        .collect();
    groups
        .chunks(3)
        .map(|row| StatRow { groups: crate::course_panel::model(row.to_vec()) })
        .collect()
}

/// mpv's short codec name, the way people write it.
pub fn codec_name(codec: &str) -> String {
    match codec {
        "h264" => "H.264".into(),
        "hevc" => "HEVC".into(),
        "av1" => "AV1".into(),
        "vp8" | "vp9" | "aac" | "ac3" | "eac3" | "opus" | "flac" | "mp3" => codec.to_uppercase(),
        other => other.to_uppercase(),
    }
}

/// Bits per second → "2.8 Mbps" / "222 kbps".
pub fn bitrate(bps: f64) -> String {
    if bps >= 1_000_000.0 {
        format!("{:.1} Mbps", bps / 1_000_000.0)
    } else {
        format!("{:.0} kbps", bps / 1000.0)
    }
}

/// Seconds → whole milliseconds ("-38ms").
pub fn milliseconds(seconds: f64) -> String {
    format!("{:.0}ms", seconds * 1000.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use slint::Model;

    #[test]
    fn formats_like_plezy() {
        assert_eq!(codec_name("h264"), "H.264");
        assert_eq!(codec_name("aac"), "AAC");
        assert_eq!(bitrate(2_800_000.0), "2.8 Mbps");
        assert_eq!(bitrate(222_000.0), "222 kbps");
        assert_eq!(milliseconds(-0.038), "-38ms");
    }

    #[test]
    fn groups_are_laid_out_three_to_a_row() {
        let groups: Groups = ["A", "B", "C", "D"].into_iter().map(|t| (t, vec![("x", "1".into())])).collect();
        let rows = rows(groups);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].groups.row_count(), 3);
        assert_eq!(rows[1].groups.row_data(0).unwrap().title.as_str(), "D");
    }
}
