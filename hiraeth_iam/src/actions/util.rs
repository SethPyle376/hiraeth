use chrono::SecondsFormat;
use hiraeth_core::{AwsActionPayloadParseError, ResolvedRequest, ServiceResponse, xml_response};
use hiraeth_store::IamStore;
use hiraeth_store::iam::Principal;
use serde::Serialize;
use uuid::Uuid;

use crate::error::IamError;

pub(super) const IAM_XMLNS: &str = "https://iam.amazonaws.com/doc/2010-05-08/";

pub(super) fn parse_payload_error(error: AwsActionPayloadParseError) -> IamError {
    match error {
        AwsActionPayloadParseError::AwsQuery(error) => IamError::from(error),
        AwsActionPayloadParseError::Json(error) => IamError::BadRequest(error.to_string()),
    }
}

pub(super) fn iam_xml_response<T: Serialize>(body: &T) -> Result<ServiceResponse, IamError> {
    xml_response(body).map_err(IamError::from)
}

pub(super) fn new_request_id() -> String {
    Uuid::new_v4().to_string()
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct ResponseMetadata {
    #[serde(rename = "RequestId")]
    pub request_id: String,
}

pub(super) fn response_metadata(request_id: impl Into<String>) -> ResponseMetadata {
    ResponseMetadata {
        request_id: request_id.into(),
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct IamUserXml {
    #[serde(rename = "Path")]
    pub path: String,
    #[serde(rename = "UserName")]
    pub user_name: String,
    #[serde(rename = "UserId")]
    pub user_id: String,
    #[serde(rename = "Arn")]
    pub arn: String,
    #[serde(rename = "CreateDate")]
    pub create_date: String,
}

impl From<Principal> for IamUserXml {
    fn from(principal: Principal) -> Self {
        IamUserXml {
            path: principal.path.clone(),
            user_name: principal.name.clone(),
            user_id: principal.user_id.clone(),
            arn: user_arn(&principal.account_id, &principal.path, &principal.name),
            create_date: principal
                .created_at
                .and_utc()
                .to_rfc3339_opts(SecondsFormat::Secs, true),
        }
    }
}

pub(super) async fn optional_target_user<S>(
    request: &ResolvedRequest,
    store: &S,
    user_name: Option<&str>,
) -> Result<Option<Principal>, IamError>
where
    S: IamStore + Send + Sync,
{
    let name = user_name.unwrap_or(&request.auth_context.principal.name);
    store
        .get_principal_by_identity(&request.auth_context.principal.account_id, "user", name)
        .await
        .map_err(IamError::from)
}

pub(super) async fn requested_or_signing_user<S>(
    request: &ResolvedRequest,
    store: &S,
    user_name: Option<&str>,
) -> Result<Principal, IamError>
where
    S: IamStore + Send + Sync,
{
    match user_name {
        Some(user_name) if user_name.trim().is_empty() => Err(IamError::BadRequest(
            "UserName must not be empty".to_string(),
        )),
        Some(user_name) => store
            .get_principal_by_identity(
                &request.auth_context.principal.account_id,
                "user",
                user_name,
            )
            .await
            .map_err(IamError::from)?
            .ok_or_else(|| IamError::NoSuchEntity(format!("User {user_name} does not exist"))),
        None if request.auth_context.principal.kind == "user" => {
            Ok(request.auth_context.principal.clone())
        }
        None => Err(IamError::BadRequest(
            "UserName is required when the caller is not an IAM user".to_string(),
        )),
    }
}

pub(super) async fn existing_user_by_name<S>(
    request: &ResolvedRequest,
    store: &S,
    user_name: &str,
) -> Result<Principal, IamError>
where
    S: IamStore + Send + Sync,
{
    store
        .get_principal_by_identity(
            &request.auth_context.principal.account_id,
            "user",
            user_name,
        )
        .await
        .map_err(IamError::from)?
        .ok_or_else(|| IamError::NoSuchEntity(format!("User with name {user_name} does not exist")))
}

pub(super) fn user_arn(account_id: &str, path: &str, user_name: &str) -> String {
    format!(
        "arn:aws:iam::{account_id}:user{}{user_name}",
        normalize_user_path(path)
    )
}

pub(super) fn normalize_user_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed == "/" {
        "/".to_string()
    } else {
        let trimmed = trimmed.trim_matches('/');
        format!("/{trimmed}/")
    }
}

pub(super) fn default_user_path() -> String {
    "/".to_string()
}
