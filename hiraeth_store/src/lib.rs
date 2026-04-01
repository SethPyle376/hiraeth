pub mod auth;
pub mod sqs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageError {
    StorageFailure(String),
}
