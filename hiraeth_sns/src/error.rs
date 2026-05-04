use std::fmt::Display;

use hiraeth_core::{
    AwsErrorFault, AwsQueryParseError, AwsServiceError, RequestBodyParseError,
    ResponseSerializationError, ServiceResponse, xml_body,
};
use hiraeth_store::StoreError;
use serde::Serialize;

const SNS_XMLNS: &str = "http://sns.amazonaws.com/doc/2010-03-31/";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnsError {
    TopicNotFound,
    SubscriptionNotFound,
    BadRequest(String),
    InternalError(String),
    UnsupportedOperation(String),
    NotAuthorizedToQueue(String),
}

impl AwsServiceError for SnsError {
    fn status_code(&self) -> u16 {
        match self {
            SnsError::InternalError(_) => 500,
            _ => 400,
        }
    }

    fn namespace(&self) -> &'static str {
        "com.amazonaws.sns"
    }

    fn code(&self) -> &'static str {
        match self {
            SnsError::TopicNotFound => "NotFound",
            SnsError::SubscriptionNotFound => "NotFound",
            SnsError::BadRequest(_) => "InvalidParameter",
            SnsError::InternalError(_) => "InternalError",
            SnsError::UnsupportedOperation(_) => "InvalidAction",
            SnsError::NotAuthorizedToQueue(_) => "AuthorizationError",
        }
    }

    fn query_error_prefix(&self) -> &'static str {
        "AWS.SimpleNotificationService"
    }

    fn fault(&self) -> AwsErrorFault {
        match self {
            SnsError::InternalError(_) => AwsErrorFault::Server,
            _ => AwsErrorFault::Sender,
        }
    }
}

impl From<AwsQueryParseError> for SnsError {
    fn from(value: AwsQueryParseError) -> Self {
        SnsError::BadRequest(value.to_string())
    }
}

impl From<RequestBodyParseError> for SnsError {
    fn from(value: RequestBodyParseError) -> Self {
        SnsError::BadRequest(value.to_string())
    }
}

impl From<ResponseSerializationError> for SnsError {
    fn from(value: ResponseSerializationError) -> Self {
        SnsError::InternalError(value.to_string())
    }
}

impl From<StoreError> for SnsError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::NotFound(_) => SnsError::TopicNotFound,
            StoreError::Conflict(msg) => SnsError::BadRequest(msg),
            StoreError::StorageFailure(msg) => SnsError::InternalError(msg),
        }
    }
}

impl Display for SnsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SnsError::TopicNotFound => write!(f, "Topic does not exist"),
            SnsError::SubscriptionNotFound => write!(f, "Subscription does not exist"),
            SnsError::BadRequest(msg) => write!(f, "{}", msg),
            SnsError::InternalError(msg) => write!(f, "{}", msg),
            SnsError::UnsupportedOperation(msg) => write!(f, "{}", msg),
            SnsError::NotAuthorizedToQueue(arn) => {
                write!(f, "SNS is not authorized to access SQS queue: {}", arn)
            }
        }
    }
}

impl From<SnsError> for ServiceResponse {
    fn from(value: SnsError) -> Self {
        let status_code = value.status_code();
        let body = xml_body(&SnsErrorResponse::from_error(&value))
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

#[derive(Debug, Serialize)]
#[serde(rename = "ErrorResponse")]
struct SnsErrorResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    #[serde(rename = "Error")]
    error: SnsErrorBody,
    #[serde(rename = "RequestId")]
    request_id: String,
}

impl SnsErrorResponse {
    fn from_error(error: &SnsError) -> Self {
        Self {
            xmlns: SNS_XMLNS,
            error: SnsErrorBody {
                error_type: error.fault().as_query_type(),
                code: error.code(),
                message: error.to_string(),
            },
            request_id: "00000000-0000-0000-0000-000000000000".to_string(),
        }
    }
}

#[derive(Debug, Serialize)]
struct SnsErrorBody {
    #[serde(rename = "Type")]
    error_type: &'static str,
    #[serde(rename = "Code")]
    code: &'static str,
    #[serde(rename = "Message")]
    message: String,
}
