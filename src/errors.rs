use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DevMateError {
    #[error("path does not exist: {0}")]
    MissingPath(PathBuf),
    #[error("expected a file: {0}")]
    ExpectedFile(PathBuf),
    #[error("expected a directory: {0}")]
    ExpectedDirectory(PathBuf),
    #[error("invalid JSON in {path} at line {line}, column {column}: {message}")]
    JsonParse {
        path: PathBuf,
        line: usize,
        column: usize,
        message: String,
    },
    #[error("not a Git repository: {0}")]
    NotGitRepository(PathBuf),
    #[error("unsupported JWT algorithm: {0}")]
    UnsupportedJwtAlgorithm(String),
}
