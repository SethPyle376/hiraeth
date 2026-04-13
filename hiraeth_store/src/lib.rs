use std::fmt::Display;

pub mod access_key_store;
pub mod principal;
pub mod sqs;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    StorageFailure(String),
}

impl Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::StorageFailure(msg) => write!(f, "Storage failure: {}", msg),
        }
    }
}
