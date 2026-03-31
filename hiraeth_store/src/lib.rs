pub mod auth;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageError {
    StorageFailure(String),
}
