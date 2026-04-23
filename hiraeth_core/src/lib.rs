mod action;
pub mod auth;
mod config;
mod protocol;
mod request;

pub use action::{AwsAction, AwsActionRegistry};
pub use config::{AuthMode, Config};
pub use protocol::{
    AwsErrorFault, AwsQueryParams, AwsQueryParseError, AwsServiceError, RequestBodyParseError,
    ResponseSerializationError, ServiceResponse, aws_batch_error_details, empty_response,
    json_body, json_response, parse_aws_query_params, parse_aws_query_request, parse_json_body,
    render_aws_json_error, render_result, xml_body, xml_response,
};
pub use request::{AuthContext, ResolvedRequest};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiError {
    NotFound(String),
    InternalServerError(String),
    BadRequest(String),
    NotAuthorized(String),
    NotAuthenticated(String),
}

impl ApiError {
    pub fn status_code(&self) -> u16 {
        match self {
            ApiError::NotFound(_) => 404,
            ApiError::InternalServerError(_) => 500,
            ApiError::BadRequest(_) => 400,
            ApiError::NotAuthorized(_) => 403,
            ApiError::NotAuthenticated(_) => 401,
        }
    }

    pub fn message(&self) -> String {
        match self {
            ApiError::NotFound(msg) => format!("Not Found: {}", msg),
            ApiError::InternalServerError(msg) => format!("Internal Server Error: {}", msg),
            ApiError::BadRequest(msg) => format!("Bad Request: {}", msg),
            ApiError::NotAuthorized(msg) => format!("Not Authorized: {}", msg),
            ApiError::NotAuthenticated(msg) => format!("Not Authenticated: {}", msg),
        }
    }
}
