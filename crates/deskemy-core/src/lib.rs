//! Deskemy's platform- and UI-agnostic core: the library database, course
//! import, config, backups and the libmpv bindings. Frontends (the Tauri app
//! today, the Slint app during the port) depend on this crate and add only
//! windowing, playback surfaces and UI on top.

pub mod backup;
pub mod config;
pub mod db;
pub mod domain;
pub mod error;
pub mod hashing;
pub mod importer;
pub mod media;
pub mod mpv;
pub mod paths;
pub mod playback;
pub mod scanner;
pub mod subtitles;
pub mod thumbnails;
