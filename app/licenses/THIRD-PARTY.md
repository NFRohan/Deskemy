# Third-party components

Deskemy itself is MIT-licensed (see `LICENSE`). It is built with, and ships,
the following:

| Component | What for | License | Text |
|---|---|---|---|
| [libmpv](https://mpv.io) (with FFmpeg), `libmpv-2.dll` | Playback | LGPL-2.1+ with GPL-2.0+ parts | `libmpv-NOTICE.txt`, `GPL-2.0.txt` |
| [Slint](https://slint.dev) | User interface | Slint Royalty-free License 2.0 (attribution in Settings → About) | <https://slint.dev/terms-and-conditions#royalty-free> |
| [Skia](https://skia.org) (via rust-skia) | Rendering | BSD-3-Clause | <https://skia.org/docs/dev/contrib/license/> |
| [Inter](https://rsms.me/inter/) | Font | SIL Open Font License 1.1 | `LICENSE-inter.txt` |
| [Lucide](https://lucide.dev) icons | Interface icons | ISC | `LICENSE-lucide.txt` |
| [Material Symbols](https://fonts.google.com/icons) | Player control icons | Apache-2.0 | `LICENSE-material-symbols.txt` |
| [SQLite](https://sqlite.org) (via rusqlite) | Library database | Public domain | — |

Deskemy's Skia renderer is a lightly patched copy of Slint's
(`i-slint-renderer-skia`, for ClearType-style text on Windows), under the same
license as Slint.

Other Rust crates compiled in are under permissive licenses (MIT, Apache-2.0,
BSD, ISC, Zlib and similar); see each crate's repository.
