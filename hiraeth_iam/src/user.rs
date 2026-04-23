use hiraeth_core::{ResolvedRequest, ServiceResponse, parse_aws_query_request};
use hiraeth_store::IamStore;
use serde::Deserialize;

use crate::error::IamError;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CreateUserRequest {
    user_name: String,
    #[serde(default = "default_user_path")]
    path: String,
    permissions_boundary: Option<String>,
}

pub(crate) async fn create_user<S: IamStore>(
    request: &ResolvedRequest,
    _store: &S,
) -> Result<ServiceResponse, IamError> {
    let create_user_request: CreateUserRequest = parse_aws_query_request(&request.request)?;
    let _ = (
        create_user_request.user_name,
        create_user_request.path,
        create_user_request.permissions_boundary,
    );

    Err(IamError::UnsupportedOperation("CreateUser".to_string()))
}

fn default_user_path() -> String {
    "/".to_string()
}
