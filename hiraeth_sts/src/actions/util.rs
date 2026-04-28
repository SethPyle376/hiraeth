use hiraeth_core::AwsActionPayloadParseError;

use crate::error::StsError;

pub(super) fn parse_payload_error(error: AwsActionPayloadParseError) -> StsError {
    match error {
        AwsActionPayloadParseError::AwsQuery(error) => StsError::BadRequest(error.to_string()),
        AwsActionPayloadParseError::Json(error) => StsError::BadRequest(error.to_string()),
    }
}
