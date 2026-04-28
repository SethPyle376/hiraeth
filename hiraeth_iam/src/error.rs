use std::fmt::Display;

use hiraeth_core::{AwsQueryParseError, ResponseSerializationError, ServiceResponse, xml_body};
use hiraeth_store::StoreError;
use serde::Serialize;
use uuid::Uuid;

const IAM_XMLNS: &str = "https://iam.amazonaws.com/doc/2010-05-08/";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum IamError {
    BadRequest(String),
    EntityAlreadyExists(String),
    NoSuchEntity(String),
    UnsupportedOperation(String),
    InternalError(String),
}

impl Display for IamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IamError::BadRequest(message) => write!(f, "{message}"),
            IamError::EntityAlreadyExists(message) => write!(f, "{message}"),
            IamError::NoSuchEntity(message) => write!(f, "{message}"),
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
        let status_code = value.status_code();
        let body = xml_body(&IamErrorResponse::from_error(&value))
            .unwrap_or_else(|_| value.to_string().into_bytes());
        ServiceResponse {
            status_code,
            headers: vec![(
                "content-type".to_string(),
                "text/xml; charset=utf-8".to_string(),
            )],
            body,
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
        match value {
            StoreError::Conflict(message) => IamError::EntityAlreadyExists(message),
            StoreError::NotFound(message) => IamError::NoSuchEntity(message),
            _ => IamError::InternalError(value.to_string()),
        }
    }
}

impl IamError {
    fn status_code(&self) -> u16 {
        match self {
            IamError::BadRequest(_) => 400,
            IamError::EntityAlreadyExists(_) => 409,
            IamError::NoSuchEntity(_) => 404,
            IamError::UnsupportedOperation(_) => 501,
            IamError::InternalError(_) => 500,
        }
    }

    fn code(&self) -> &'static str {
        match self {
            IamError::BadRequest(_) => "ValidationError",
            IamError::EntityAlreadyExists(_) => "EntityAlreadyExists",
            IamError::NoSuchEntity(_) => "NoSuchEntity",
            IamError::UnsupportedOperation(_) => "InvalidAction",
            IamError::InternalError(_) => "InternalFailure",
        }
    }

    fn error_type(&self) -> &'static str {
        match self {
            IamError::InternalError(_) => "Server",
            _ => "Sender",
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename = "ErrorResponse")]
struct IamErrorResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    #[serde(rename = "Error")]
    error: IamErrorBody,
    #[serde(rename = "RequestId")]
    request_id: String,
}

impl IamErrorResponse {
    fn from_error(error: &IamError) -> Self {
        Self {
            xmlns: IAM_XMLNS,
            error: IamErrorBody {
                error_type: error.error_type(),
                code: error.code(),
                message: error.to_string(),
            },
            request_id: Uuid::new_v4().to_string(),
        }
    }
}

#[derive(Debug, Serialize)]
struct IamErrorBody {
    #[serde(rename = "Type")]
    error_type: &'static str,
    #[serde(rename = "Code")]
    code: &'static str,
    #[serde(rename = "Message")]
    message: String,
}

#[cfg(test)]
mod tests {
    use super::{IamError, IamErrorResponse};
    use hiraeth_core::xml_body;

    #[test]
    fn iam_error_response_serializes_query_error_shape() {
        let xml = String::from_utf8(
            xml_body(&IamErrorResponse::from_error(&IamError::NoSuchEntity(
                "User test does not exist".to_string(),
            )))
            .expect("error xml should serialize"),
        )
        .expect("error xml should be utf8");

        assert!(
            xml.contains(r#"<ErrorResponse xmlns="https://iam.amazonaws.com/doc/2010-05-08/">"#)
        );
        assert!(xml.contains("<Type>Sender</Type>"));
        assert!(xml.contains("<Code>NoSuchEntity</Code>"));
        assert!(xml.contains("<Message>User test does not exist</Message>"));
        assert!(xml.contains("<RequestId>"));
    }
}
