use std::fmt::{Debug, Display};

use serde::{Serialize, de::DeserializeOwned};

use crate::ApiError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceResponse {
    pub status_code: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AwsErrorFault {
    Sender,
    Server,
}

impl AwsErrorFault {
    pub fn as_query_type(self) -> &'static str {
        match self {
            AwsErrorFault::Sender => "Sender",
            AwsErrorFault::Server => "Server",
        }
    }

    pub fn sender_fault(self) -> bool {
        matches!(self, AwsErrorFault::Sender)
    }
}

pub trait AwsServiceError: Display {
    fn status_code(&self) -> u16;

    fn namespace(&self) -> &'static str;

    fn code(&self) -> &'static str;

    fn query_error_prefix(&self) -> &'static str;

    fn fault(&self) -> AwsErrorFault;

    fn query_code(&self) -> &'static str {
        self.code()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct AwsJsonErrorBody {
    #[serde(rename = "__type")]
    error_type: String,
    message: String,
}

pub fn render_aws_json_error<E: AwsServiceError>(error: &E) -> ServiceResponse {
    let body = AwsJsonErrorBody {
        error_type: format!("{}#{}", error.namespace(), error.code()),
        message: error.to_string(),
    };

    ServiceResponse {
        status_code: error.status_code(),
        headers: vec![
            (
                "content-type".to_string(),
                "application/x-amz-json-1.0".to_string(),
            ),
            (
                "x-amzn-query-error".to_string(),
                format!(
                    "{}.{};{}",
                    error.query_error_prefix(),
                    error.query_code(),
                    error.fault().as_query_type()
                ),
            ),
        ],
        body: serde_json::to_vec(&body).unwrap_or_else(|_| vec![]),
    }
}

pub fn aws_batch_error_details<E: AwsServiceError>(error: &E) -> (&'static str, bool) {
    (error.code(), error.fault().sender_fault())
}

pub fn empty_response() -> ServiceResponse {
    ServiceResponse {
        status_code: 200,
        headers: vec![],
        body: vec![],
    }
}

pub fn json_response<T: Serialize>(
    body: &T,
) -> Result<ServiceResponse, ResponseSerializationError> {
    Ok(ServiceResponse {
        status_code: 200,
        headers: vec![],
        body: json_body(body)?,
    })
}

pub fn json_body<T: Serialize>(body: &T) -> Result<Vec<u8>, ResponseSerializationError> {
    serde_json::to_vec(body).map_err(ResponseSerializationError::new)
}

pub fn parse_json_body<T: DeserializeOwned>(body: &[u8]) -> Result<T, RequestBodyParseError> {
    serde_json::from_slice(body).map_err(RequestBodyParseError::new)
}

pub fn render_result<T, E>(result: Result<T, E>) -> ServiceResponse
where
    T: Into<ServiceResponse>,
    E: Into<ServiceResponse>,
{
    match result {
        Ok(success) => success.into(),
        Err(error) => error.into(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseSerializationError {
    message: String,
}

impl ResponseSerializationError {
    fn new(error: serde_json::Error) -> Self {
        Self {
            message: format!("failed to serialize response: {}", error),
        }
    }
}

impl Display for ResponseSerializationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ResponseSerializationError {}

impl From<ResponseSerializationError> for ApiError {
    fn from(value: ResponseSerializationError) -> Self {
        ApiError::InternalServerError(value.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestBodyParseError {
    message: String,
}

impl RequestBodyParseError {
    fn new(error: serde_json::Error) -> Self {
        Self {
            message: format!("Invalid request body: {}", error),
        }
    }
}

impl Display for RequestBodyParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for RequestBodyParseError {}

impl From<RequestBodyParseError> for ApiError {
    fn from(value: RequestBodyParseError) -> Self {
        ApiError::BadRequest(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AwsErrorFault, AwsServiceError, ServiceResponse, aws_batch_error_details, empty_response,
        json_response, parse_json_body, render_aws_json_error, render_result,
    };

    #[derive(Debug)]
    struct TestError;

    impl std::fmt::Display for TestError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "test message")
        }
    }

    impl AwsServiceError for TestError {
        fn status_code(&self) -> u16 {
            400
        }

        fn namespace(&self) -> &'static str {
            "com.amazonaws.test"
        }

        fn code(&self) -> &'static str {
            "TestError"
        }

        fn query_error_prefix(&self) -> &'static str {
            "AWS.TestService"
        }

        fn fault(&self) -> AwsErrorFault {
            AwsErrorFault::Sender
        }

        fn query_code(&self) -> &'static str {
            "DifferentQueryError"
        }
    }

    impl From<TestError> for ServiceResponse {
        fn from(value: TestError) -> Self {
            render_aws_json_error(&value)
        }
    }

    #[test]
    fn renders_aws_json_error_response() {
        let response = render_aws_json_error(&TestError);
        let body: serde_json::Value =
            serde_json::from_slice(&response.body).expect("error response should be json");

        assert_eq!(response.status_code, 400);
        assert_eq!(
            response
                .headers
                .iter()
                .find(|(name, _)| name == "x-amzn-query-error")
                .map(|(_, value)| value.as_str()),
            Some("AWS.TestService.DifferentQueryError;Sender")
        );
        assert_eq!(body["__type"], "com.amazonaws.test#TestError");
        assert_eq!(body["message"], "test message");
    }

    #[test]
    fn exposes_batch_error_details_from_service_error_metadata() {
        assert_eq!(aws_batch_error_details(&TestError), ("TestError", true));
    }

    #[test]
    fn builds_empty_success_response() {
        let response = empty_response();

        assert_eq!(response.status_code, 200);
        assert!(response.headers.is_empty());
        assert!(response.body.is_empty());
    }

    #[test]
    fn builds_json_success_response() {
        let response = json_response(&serde_json::json!({"ok": true}))
            .expect("json response should serialize");
        let body: serde_json::Value =
            serde_json::from_slice(&response.body).expect("response body should be json");

        assert_eq!(response.status_code, 200);
        assert_eq!(body["ok"], true);
    }

    #[test]
    fn parses_json_body_with_consistent_error_message() {
        let error = parse_json_body::<serde_json::Value>(b"{").expect_err("body should fail");

        assert!(error.to_string().starts_with("Invalid request body:"));
    }

    #[test]
    fn renders_result_with_success_or_error_response() {
        let success = render_result::<_, TestError>(Ok(empty_response()));
        let failure = render_result::<ServiceResponse, _>(Err(TestError));

        assert_eq!(success.status_code, 200);
        assert_eq!(failure.status_code, 400);
    }
}
