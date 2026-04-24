use hiraeth_core::{ApiError, AwsActionPayloadFormat, AwsActionPayloadParseError, ServiceResponse};

use crate::error::SqsError;

pub(super) fn json_payload_format() -> AwsActionPayloadFormat {
    AwsActionPayloadFormat::Json
}

pub(super) fn parse_payload_error(error: AwsActionPayloadParseError) -> ServiceResponse {
    let error = match error {
        AwsActionPayloadParseError::Json(error) => SqsError::from(error),
        AwsActionPayloadParseError::AwsQuery(error) => SqsError::BadRequest(error.to_string()),
    };

    ServiceResponse::from(error)
}

pub(super) fn render_result<E>(
    result: Result<ServiceResponse, E>,
) -> Result<ServiceResponse, ApiError>
where
    E: Into<ServiceResponse>,
{
    match result {
        Ok(response) => Ok(response),
        Err(error) => Ok(error.into()),
    }
}
