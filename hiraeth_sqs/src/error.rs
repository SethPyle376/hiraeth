use std::fmt::Display;

use hiraeth_core::{
    AwsErrorFault, AwsServiceError, RequestBodyParseError, ResponseSerializationError,
    ServiceResponse, aws_batch_error_details, render_aws_json_error,
};
use hiraeth_store::StoreError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SqsError {
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

impl AwsServiceError for SqsError {
    fn status_code(&self) -> u16 {
        match self {
            SqsError::InternalError(_) => 500,
            _ => 400,
        }
    }

    fn namespace(&self) -> &'static str {
        "com.amazonaws.sqs"
    }

    fn code(&self) -> &'static str {
        match self {
            SqsError::QueueNotFound => "QueueDoesNotExist",
            SqsError::BadRequest(_) => "InvalidParameterValue",
            SqsError::BatchEntryIdsNotDistinct => "BatchEntryIdsNotDistinct",
            SqsError::EmptyBatchRequest => "EmptyBatchRequest",
            SqsError::InternalError(_) => "InternalError",
            SqsError::QueueAlreadyExists(_) => "QueueAlreadyExists",
            SqsError::ReceiptHandleIsInvalid(_) => "ReceiptHandleIsInvalid",
            SqsError::TooManyEntriesInBatchRequest => "TooManyEntriesInBatchRequest",
            SqsError::UnsupportedOperation(_) => "UnsupportedOperation",
        }
    }

    fn query_error_prefix(&self) -> &'static str {
        "AWS.SimpleQueueService"
    }

    fn fault(&self) -> AwsErrorFault {
        match self {
            SqsError::InternalError(_) => AwsErrorFault::Server,
            _ => AwsErrorFault::Sender,
        }
    }

    fn query_code(&self) -> &'static str {
        match self {
            SqsError::QueueNotFound => "NonExistentQueue",
            _ => self.code(),
        }
    }
}

impl From<RequestBodyParseError> for SqsError {
    fn from(value: RequestBodyParseError) -> Self {
        SqsError::BadRequest(value.to_string())
    }
}

impl From<ResponseSerializationError> for SqsError {
    fn from(value: ResponseSerializationError) -> Self {
        SqsError::InternalError(value.to_string())
    }
}

pub fn map_store_error(error: StoreError) -> SqsError {
    match error {
        StoreError::NotFound(_) => SqsError::QueueNotFound,
        StoreError::Conflict(msg) => SqsError::BadRequest(msg),
        StoreError::StorageFailure(msg) => SqsError::InternalError(msg),
    }
}

pub fn map_receipt_handle_store_error(error: StoreError) -> SqsError {
    match error {
        StoreError::NotFound(msg) => SqsError::ReceiptHandleIsInvalid(msg),
        StoreError::Conflict(msg) => SqsError::BadRequest(msg),
        StoreError::StorageFailure(msg) => SqsError::InternalError(msg),
    }
}

pub fn batch_error_details(error: &SqsError) -> (&'static str, bool) {
    aws_batch_error_details(error)
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

impl From<SqsError> for ServiceResponse {
    fn from(value: SqsError) -> Self {
        render_aws_json_error(&value)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::{SqsError, batch_error_details};
    use hiraeth_core::ServiceResponse;

    fn parse_json_body(response: &ServiceResponse) -> Value {
        serde_json::from_slice(&response.body).expect("response body should be valid json")
    }

    #[test]
    fn renders_queue_not_found_with_sdk_compatible_query_error() {
        let response: ServiceResponse = SqsError::QueueNotFound.into();
        let body = parse_json_body(&response);

        assert_eq!(response.status_code, 400);
        assert_eq!(
            response
                .headers
                .iter()
                .find(|(name, _)| name == "x-amzn-query-error")
                .map(|(_, value)| value.as_str()),
            Some("AWS.SimpleQueueService.NonExistentQueue;Sender")
        );
        assert_eq!(body["__type"], "com.amazonaws.sqs#QueueDoesNotExist");
        assert_eq!(body["message"], "The specified queue does not exist.");
    }

    #[test]
    fn renders_internal_errors_as_server_faults() {
        let response: ServiceResponse = SqsError::InternalError("boom".to_string()).into();
        let body = parse_json_body(&response);

        assert_eq!(response.status_code, 500);
        assert_eq!(
            response
                .headers
                .iter()
                .find(|(name, _)| name == "x-amzn-query-error")
                .map(|(_, value)| value.as_str()),
            Some("AWS.SimpleQueueService.InternalError;Server")
        );
        assert_eq!(body["__type"], "com.amazonaws.sqs#InternalError");
        assert_eq!(body["message"], "boom");
    }

    #[test]
    fn batch_error_details_use_same_error_metadata() {
        assert_eq!(
            batch_error_details(&SqsError::BadRequest("invalid".to_string())),
            ("InvalidParameterValue", true)
        );
        assert_eq!(
            batch_error_details(&SqsError::InternalError("boom".to_string())),
            ("InternalError", false)
        );
    }
}
