use std::fmt::Display;

use hiraeth_router::ServiceResponse;
use hiraeth_store::StoreError;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SqsError {
    QueueNotFound,
    BadRequest(String),
    BatchEntryIdsNotDistinct,
    EmptyBatchRequest,
    InternalError(String),
    QueueAlreadyExists(String),
    ReceiptHandleIsInvalid(String),
    TooManyEntriesInBatchRequest,
    UnsupportedOperation(String),
}

pub(crate) fn map_store_error(error: StoreError) -> SqsError {
    match error {
        StoreError::NotFound(_) => SqsError::QueueNotFound,
        StoreError::Conflict(msg) => SqsError::BadRequest(msg),
        StoreError::StorageFailure(msg) => SqsError::InternalError(msg),
    }
}

pub(crate) fn map_receipt_handle_store_error(error: StoreError) -> SqsError {
    match error {
        StoreError::NotFound(msg) => SqsError::ReceiptHandleIsInvalid(msg),
        StoreError::Conflict(msg) => SqsError::BadRequest(msg),
        StoreError::StorageFailure(msg) => SqsError::InternalError(msg),
    }
}

pub(crate) fn batch_error_details(error: &SqsError) -> (&'static str, bool) {
    match error {
        SqsError::BadRequest(_) => ("InvalidParameterValue", true),
        SqsError::InternalError(_) => ("InternalError", false),
        SqsError::ReceiptHandleIsInvalid(_) => ("ReceiptHandleIsInvalid", true),
        _ => ("InternalError", false),
    }
}

impl Display for SqsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SqsError::QueueNotFound => write!(f, "The specified queue does not exist."),
            SqsError::BadRequest(msg) => write!(f, "{}", msg),
            SqsError::BatchEntryIdsNotDistinct => write!(
                f,
                "Two or more batch entries in the request have the same Id."
            ),
            SqsError::EmptyBatchRequest => {
                write!(f, "The batch request must contain at least one entry.")
            }
            SqsError::InternalError(msg) => write!(f, "{}", msg),
            SqsError::QueueAlreadyExists(msg) => write!(f, "{}", msg),
            SqsError::ReceiptHandleIsInvalid(msg) => write!(f, "{}", msg),
            SqsError::TooManyEntriesInBatchRequest => {
                write!(f, "The batch request contains more entries than allowed.")
            }
            SqsError::UnsupportedOperation(msg) => write!(f, "{}", msg),
        }
    }
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
            SqsError::BatchEntryIdsNotDistinct => ErrorResponse {
                status_code: 400,
                body: ErrorResponseBody {
                    error_type: "com.amazonaws.sqs#BatchEntryIdsNotDistinct".to_string(),
                    message: "Two or more batch entries in the request have the same Id."
                        .to_string(),
                },
                query_error: "AWS.SimpleQueueService.BatchEntryIdsNotDistinct;Sender".to_string(),
            },
            SqsError::EmptyBatchRequest => ErrorResponse {
                status_code: 400,
                body: ErrorResponseBody {
                    error_type: "com.amazonaws.sqs#EmptyBatchRequest".to_string(),
                    message: "The batch request must contain at least one entry.".to_string(),
                },
                query_error: "AWS.SimpleQueueService.EmptyBatchRequest;Sender".to_string(),
            },
            SqsError::InternalError(msg) => ErrorResponse {
                status_code: 500,
                body: ErrorResponseBody {
                    error_type: "com.amazonaws.sqs#InternalError".to_string(),
                    message: msg,
                },
                query_error: "AWS.SimpleQueueService.InternalError;Server".to_string(),
            },
            SqsError::QueueAlreadyExists(msg) => ErrorResponse {
                status_code: 400,
                body: ErrorResponseBody {
                    error_type: "com.amazonaws.sqs#QueueAlreadyExists".to_string(),
                    message: msg,
                },
                query_error: "AWS.SimpleQueueService.QueueAlreadyExists;Sender".to_string(),
            },
            SqsError::ReceiptHandleIsInvalid(msg) => ErrorResponse {
                status_code: 400,
                body: ErrorResponseBody {
                    error_type: "com.amazonaws.sqs#ReceiptHandleIsInvalid".to_string(),
                    message: msg,
                },
                query_error: "AWS.SimpleQueueService.ReceiptHandleIsInvalid;Sender".to_string(),
            },
            SqsError::TooManyEntriesInBatchRequest => ErrorResponse {
                status_code: 400,
                body: ErrorResponseBody {
                    error_type: "com.amazonaws.sqs#TooManyEntriesInBatchRequest".to_string(),
                    message: "The batch request contains more entries than allowed.".to_string(),
                },
                query_error: "AWS.SimpleQueueService.TooManyEntriesInBatchRequest;Sender"
                    .to_string(),
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
