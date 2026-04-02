pub mod access_key_store;
pub mod principal;
pub mod sqs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageError {
    StorageFailure(String),
}
