//! Database error types.

/// Database operation errors
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    /// SQL error from sqlx
    #[error("SQL error: {0}")]
    Sql(#[from] sqlx::Error),

    /// User not found
    #[error("User not found: {0}")]
    UserNotFound(String),

    /// Invalid status transition
    #[error("Invalid status transition from {from} to {to}")]
    InvalidTransition { from: String, to: String },

    /// Config directory not found
    #[error("Config/data directory not found")]
    NoConfigDir,

    /// Migration error
    #[error("Migration error: {0}")]
    Migration(String),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// SQLite vec initialization error
    #[error("SQLite-vec initialization error: {0}")]
    SqliteVec(String),
}

/// Result type alias for database operations
pub type DbResult<T> = Result<T, DbError>;
