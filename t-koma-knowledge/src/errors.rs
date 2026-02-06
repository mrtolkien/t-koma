use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum KnowledgeError {
    #[error("missing data directory")]
    MissingDataDir,
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("toml parse error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("sqlite error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("migration error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("sqlite-vec initialization error: {0}")]
    SqliteVec(String),
    #[error("notify error: {0}")]
    Notify(#[from] notify::Error),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("invalid front matter: {0}")]
    InvalidFrontMatter(String),
    #[error("missing required field: {0}")]
    MissingField(&'static str),
    #[error("unsupported language for code chunking: {0}")]
    UnsupportedLanguage(String),
    #[error("embedding dimension mismatch: expected {expected}, got {actual}")]
    EmbeddingDimMismatch { expected: usize, actual: usize },
    #[error("unknown note: {0}")]
    UnknownNote(String),
    #[error("path outside allowed root: {0}")]
    PathOutsideRoot(PathBuf),
    #[error("embedding error: {0}")]
    Embedding(String),
    #[error("access denied: {0}")]
    AccessDenied(String),
}

pub type KnowledgeResult<T> = Result<T, KnowledgeError>;
