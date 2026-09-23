//! Crate-wide error type. Every command returns `Result<T, DeskemyError>`.
//! Serializes to `{ kind, message }` so the frontend can branch on `kind`
//! instead of parsing strings.

use serde::ser::{Serialize, SerializeStruct, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum DeskemyError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("archive error: {0}")]
    Archive(#[from] zip::result::ZipError),

    #[error("config error: {0}")]
    Config(String),

    #[error("import error: {0}")]
    Import(String),

    #[error("probe error: {0}")]
    Probe(String),

    #[error("player error: {0}")]
    Player(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("{0}")]
    Other(String),
}

impl DeskemyError {
    /// Stable machine-readable discriminant sent to the frontend.
    pub fn kind(&self) -> &'static str {
        match self {
            DeskemyError::Io(_) => "Io",
            DeskemyError::Database(_) => "Database",
            DeskemyError::Serde(_) => "Serde",
            DeskemyError::Archive(_) => "Archive",
            DeskemyError::Config(_) => "Config",
            DeskemyError::Import(_) => "Import",
            DeskemyError::Probe(_) => "Probe",
            DeskemyError::Player(_) => "Player",
            DeskemyError::NotFound(_) => "NotFound",
            DeskemyError::Other(_) => "Other",
        }
    }
}

impl Serialize for DeskemyError {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut st = serializer.serialize_struct("DeskemyError", 2)?;
        st.serialize_field("kind", self.kind())?;
        st.serialize_field("message", &self.to_string())?;
        st.end()
    }
}

pub type Result<T> = std::result::Result<T, DeskemyError>;
