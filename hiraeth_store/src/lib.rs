use std::fmt::Display;

pub mod iam;
pub mod sns;

pub mod access_key_store;
pub mod principal;
pub mod sqs;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub use iam::IamStore;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    NotFound(String),
    Conflict(String),
    StorageFailure(String),
}

impl Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::NotFound(msg) => write!(f, "Not found: {}", msg),
            StoreError::Conflict(msg) => write!(f, "Conflict: {}", msg),
            StoreError::StorageFailure(msg) => write!(f, "Storage failure: {}", msg),
        }
    }
}

impl std::error::Error for StoreError {}
