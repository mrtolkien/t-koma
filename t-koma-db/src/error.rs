//! Database error types.

/// Database operation errors
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    /// SQL error from sqlx
    #[error("SQL error: {0}")]
    Sql(#[from] sqlx::Error),

    /// Operator not found
    #[error("Operator not found: {0}")]
    OperatorNotFound(String),

    /// Ghost not found
    #[error("Ghost not found: {0}")]
    GhostNotFound(String),

    /// Ghost name already exists
    #[error("Ghost name already exists: {0}")]
    GhostNameTaken(String),

    /// Invalid ghost name
    #[error("Invalid ghost name: {0}")]
    InvalidGhostName(String),

    /// Interface not found
    #[error("Interface not found: {0}")]
    InterfaceNotFound(String),

    /// Interface already exists
    #[error("Interface already exists: {0}")]
    InterfaceAlreadyExists(String),

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

    /// Session not found
    #[error("Session not found: {0}")]
    SessionNotFound(String),

    /// Unauthorized access
    #[error("Unauthorized access")]
    Unauthorized,

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Invalid role
    #[error("Invalid role: {0}")]
    InvalidRole(String),
}

/// Result type alias for database operations
pub type DbResult<T> = Result<T, DbError>;
