use std::fmt::Display;

use hiraeth_core::{AwsQueryParseError, ResponseSerializationError, ServiceResponse};
use hiraeth_store::StoreError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum IamError {
    BadRequest(String),
    UnsupportedOperation(String),
    InternalError(String),
}

impl Display for IamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IamError::BadRequest(message) => write!(f, "{message}"),
            IamError::UnsupportedOperation(action) => {
                write!(f, "IAM action {action} is not implemented")
            }
            IamError::InternalError(message) => write!(f, "{message}"),
        }
    }
}

impl From<AwsQueryParseError> for IamError {
    fn from(value: AwsQueryParseError) -> Self {
        IamError::BadRequest(value.to_string())
    }
}

impl From<IamError> for ServiceResponse {
    fn from(value: IamError) -> Self {
        let status_code = match value {
            IamError::BadRequest(_) => 400,
            IamError::UnsupportedOperation(_) => 501,
            IamError::InternalError(_) => 500,
        };

        ServiceResponse {
            status_code,
            headers: vec![(
                "content-type".to_string(),
                "text/plain; charset=utf-8".to_string(),
            )],
            body: value.to_string().into_bytes(),
        }
    }
}

impl From<ResponseSerializationError> for IamError {
    fn from(value: ResponseSerializationError) -> Self {
        IamError::InternalError(value.to_string())
    }
}

impl From<StoreError> for IamError {
    fn from(value: StoreError) -> Self {
        IamError::InternalError(value.to_string())
    }
}
