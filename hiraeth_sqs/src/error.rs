use hiraeth_router::ServiceResponse;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SqsError {
    QueueNotFound,
    BadRequest(String),
    InternalError(String),
    UnsupportedOperation(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ErrorResponse {
    status_code: u16,
    body: ErrorResponseBody,
    query_error: String,
}

impl From<ErrorResponse> for ServiceResponse {
    fn from(value: ErrorResponse) -> Self {
        ServiceResponse {
            status_code: value.status_code,
            headers: vec![
                (
                    "content-type".to_string(),
                    "application/x-amz-json-1.0".to_string(),
                ),
                ("x-amzn-query-error".to_string(), value.query_error),
            ],
            body: serde_json::to_vec(&value.body).unwrap_or_else(|_| vec![]),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ErrorResponseBody {
    #[serde(rename = "__type")]
    error_type: String,
    message: String,
}

impl Into<ServiceResponse> for SqsError {
    fn into(self) -> ServiceResponse {
        let error_response = match self {
            SqsError::QueueNotFound => ErrorResponse {
                status_code: 400,
                body: ErrorResponseBody {
                    error_type: "com.amazonaws.sqs#QueueDoesNotExist".to_string(),
                    message: "The specified queue does not exist.".to_string(),
                },
                query_error: "AWS.SimpleQueueService.NonExistentQueue;Sender".to_string(),
            },
            SqsError::BadRequest(msg) => ErrorResponse {
                status_code: 400,
                body: ErrorResponseBody {
                    error_type: "com.amazonaws.sqs#InvalidParameterValue".to_string(),
                    message: msg,
                },
                query_error: "AWS.SimpleQueueService.InvalidParameterValue;Sender".to_string(),
            },
            SqsError::InternalError(msg) => ErrorResponse {
                status_code: 500,
                body: ErrorResponseBody {
                    error_type: "com.amazonaws.sqs#InternalError".to_string(),
                    message: msg,
                },
                query_error: "AWS.SimpleQueueService.InternalError;Server".to_string(),
            },
            SqsError::UnsupportedOperation(msg) => ErrorResponse {
                status_code: 400,
                body: ErrorResponseBody {
                    error_type: "com.amazonaws.sqs#UnsupportedOperation".to_string(),
                    message: msg,
                },
                query_error: "AWS.SimpleQueueService.UnsupportedOperation;Sender".to_string(),
            },
        };

        error_response.into()
    }
}

pub(crate) fn render_result<T: Into<ServiceResponse>>(
    result: Result<T, SqsError>,
) -> ServiceResponse {
    match result {
        Ok(success) => success.into(),
        Err(error) => error.into(),
    }
}
