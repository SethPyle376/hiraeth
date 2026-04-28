use std::fmt::Display;

use hiraeth_core::{AwsQueryParseError, ResponseSerializationError, ServiceResponse};
use hiraeth_store::StoreError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StsError {
    BadRequest(String),
    UnsupportedOperation(String),
    InternalError(String),
}

impl Display for StsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StsError::BadRequest(message) => write!(f, "{message}"),
            StsError::UnsupportedOperation(action) => {
                write!(f, "STS action {action} is not implemented")
            }
            StsError::InternalError(message) => write!(f, "{message}"),
        }
    }
}

impl From<AwsQueryParseError> for StsError {
    fn from(value: AwsQueryParseError) -> Self {
        StsError::BadRequest(value.to_string())
    }
}

impl From<StsError> for ServiceResponse {
    fn from(value: StsError) -> Self {
        let status_code = match value {
            StsError::BadRequest(_) => 400,
            StsError::UnsupportedOperation(_) => 501,
            StsError::InternalError(_) => 500,
        };
        ServiceResponse {
            status_code,
            headers: vec![(
                "content-type".to_string(),
                "text/xml; charset=utf-8".to_string(),
            )],
            body: value.to_string().into_bytes(),
        }
    }
}

impl From<ResponseSerializationError> for StsError {
    fn from(value: ResponseSerializationError) -> Self {
        StsError::InternalError(value.to_string())
    }
}

impl From<StoreError> for StsError {
    fn from(value: StoreError) -> Self {
        StsError::InternalError(value.to_string())
    }
}
