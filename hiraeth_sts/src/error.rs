use std::fmt::Display;

use hiraeth_core::{AwsQueryParseError, ResponseSerializationError, ServiceResponse, xml_body};
use hiraeth_store::StoreError;
use serde::Serialize;

const STS_XMLNS: &str = "https://sts.amazonaws.com/doc/2011-06-15/";
const PLACEHOLDER_REQUEST_ID: &str = "00000000-0000-0000-0000-000000000000";

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
        let status_code = value.status_code();
        let body = xml_body(&StsErrorResponse::from_error(&value))
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

impl StsError {
    fn status_code(&self) -> u16 {
        match self {
            StsError::BadRequest(_) => 400,
            StsError::UnsupportedOperation(_) => 501,
            StsError::InternalError(_) => 500,
        }
    }

    fn code(&self) -> &'static str {
        match self {
            StsError::BadRequest(_) => "ValidationError",
            StsError::UnsupportedOperation(_) => "InvalidAction",
            StsError::InternalError(_) => "InternalFailure",
        }
    }

    fn error_type(&self) -> &'static str {
        match self {
            StsError::InternalError(_) => "Server",
            _ => "Sender",
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename = "ErrorResponse")]
struct StsErrorResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    #[serde(rename = "Error")]
    error: StsErrorBody,
    #[serde(rename = "RequestId")]
    request_id: String,
}

impl StsErrorResponse {
    fn from_error(error: &StsError) -> Self {
        Self {
            xmlns: STS_XMLNS,
            error: StsErrorBody {
                error_type: error.error_type(),
                code: error.code(),
                message: error.to_string(),
            },
            request_id: PLACEHOLDER_REQUEST_ID.to_string(),
        }
    }
}

#[derive(Debug, Serialize)]
struct StsErrorBody {
    #[serde(rename = "Type")]
    error_type: &'static str,
    #[serde(rename = "Code")]
    code: &'static str,
    #[serde(rename = "Message")]
    message: String,
}

#[cfg(test)]
mod tests {
    use super::StsError;
    use hiraeth_core::ServiceResponse;

    #[test]
    fn renders_sts_query_error_xml_shape() {
        let response: ServiceResponse =
            StsError::BadRequest("Missing Action parameter".to_string()).into();
        let body = String::from_utf8(response.body).expect("response body should be utf8");

        assert_eq!(response.status_code, 400);
        assert_eq!(
            response
                .headers
                .iter()
                .find(|(name, _)| name == "content-type")
                .map(|(_, value)| value.as_str()),
            Some("text/xml; charset=utf-8")
        );
        assert!(
            body.contains(r#"<ErrorResponse xmlns="https://sts.amazonaws.com/doc/2011-06-15/">"#)
        );
        assert!(body.contains("<Type>Sender</Type>"));
        assert!(body.contains("<Code>ValidationError</Code>"));
        assert!(body.contains("<Message>Missing Action parameter</Message>"));
        assert!(body.contains("<RequestId>"));
    }

    #[test]
    fn renders_internal_errors_as_server_faults() {
        let response: ServiceResponse = StsError::InternalError("boom".to_string()).into();
        let body = String::from_utf8(response.body).expect("response body should be utf8");

        assert_eq!(response.status_code, 500);
        assert!(body.contains("<Type>Server</Type>"));
        assert!(body.contains("<Code>InternalFailure</Code>"));
        assert!(body.contains("<Message>boom</Message>"));
    }
}
