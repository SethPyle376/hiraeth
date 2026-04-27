use hiraeth_core::{AwsActionPayloadFormat, AwsActionPayloadParseError};

use crate::error::SqsError;

pub(super) fn json_payload_format() -> AwsActionPayloadFormat {
    AwsActionPayloadFormat::Json
}

pub(super) fn parse_payload_error(error: AwsActionPayloadParseError) -> SqsError {
    match error {
        AwsActionPayloadParseError::Json(error) => SqsError::from(error),
        AwsActionPayloadParseError::AwsQuery(error) => SqsError::BadRequest(error.to_string()),
    }
}
