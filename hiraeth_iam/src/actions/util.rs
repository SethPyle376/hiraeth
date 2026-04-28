use chrono::SecondsFormat;
use hiraeth_core::auth::Policy;
use hiraeth_core::{
    AwsActionPayloadParseError, ResolvedRequest, ServiceResponse, arn_util, xml_response,
};
use hiraeth_store::IamStore;
use hiraeth_store::iam::{ManagedPolicy, Principal};
use serde::Serialize;
use uuid::Uuid;

use crate::error::IamError;

pub(super) const IAM_XMLNS: &str = "https://iam.amazonaws.com/doc/2010-05-08/";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PolicyArn {
    pub account_id: String,
    pub policy_path: String,
    pub policy_name: String,
}

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

pub(super) fn new_id() -> String {
    format!("AIDA{}", Uuid::new_v4().simple().to_string().to_uppercase())
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
            arn: arn_util::user_arn(&principal.account_id, &principal.path, &principal.name),
            create_date: principal
                .created_at
                .and_utc()
                .to_rfc3339_opts(SecondsFormat::Secs, true),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct IamPolicyXml {
    pub path: Option<String>,
    pub policy_name: Option<String>,
    pub default_version_id: Option<String>,
    pub policy_id: Option<String>,
    pub arn: Option<String>,
    pub attachments_count: Option<i64>,
    pub create_date: Option<String>,
    pub update_date: Option<String>,
}

impl From<ManagedPolicy> for IamPolicyXml {
    fn from(policy: ManagedPolicy) -> Self {
        let policy_path = normalize_policy_path(policy.policy_path.as_deref());
        IamPolicyXml {
            path: Some(policy_path.clone()),
            policy_name: Some(policy.policy_name.clone()),
            default_version_id: None,
            policy_id: Some(policy.policy_id.clone()),
            arn: Some(arn_util::policy_arn(
                &policy.account_id,
                &policy_path,
                &policy.policy_name,
            )),
            attachments_count: None,
            create_date: Some(
                policy
                    .created_at
                    .and_utc()
                    .to_rfc3339_opts(SecondsFormat::Secs, true),
            ),
            update_date: Some(
                policy
                    .updated_at
                    .and_utc()
                    .to_rfc3339_opts(SecondsFormat::Secs, true),
            ),
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

pub(super) fn parse_policy_arn(arn: &str) -> Result<PolicyArn, IamError> {
    let parts: Vec<&str> = arn.split(':').collect();
    if parts.len() != 6 || parts[0] != "arn" || parts[1] != "aws" || parts[2] != "iam" {
        return Err(IamError::BadRequest(format!("Invalid ARN format: {arn}")));
    }

    let account_id = parts[4].to_string();
    let resource = parts[5];
    let resource_parts: Vec<&str> = resource.split('/').collect();
    if resource_parts.len() < 2 || resource_parts[0] != "policy" {
        return Err(IamError::BadRequest(format!(
            "Invalid policy ARN format: {arn}"
        )));
    }

    let policy_name = resource_parts[resource_parts.len() - 1];
    if policy_name.is_empty() {
        return Err(IamError::BadRequest(format!(
            "Invalid policy ARN format: {arn}"
        )));
    }
    let policy_path = if resource_parts.len() == 2 {
        "/".to_string()
    } else {
        format!(
            "/{}/",
            resource_parts[1..resource_parts.len() - 1].join("/")
        )
    };

    Ok(PolicyArn {
        account_id,
        policy_path,
        policy_name: policy_name.to_string(),
    })
}

pub(super) fn default_user_path() -> String {
    "/".to_string()
}

pub(super) fn normalize_policy_path(path: Option<&str>) -> String {
    match path {
        Some(path) if !path.trim().is_empty() => {
            let trimmed = path.trim();
            let with_leading = if trimmed.starts_with('/') {
                trimmed.to_string()
            } else {
                format!("/{trimmed}")
            };
            if with_leading.ends_with('/') {
                with_leading
            } else {
                format!("{with_leading}/")
            }
        }
        _ => "/".to_string(),
    }
}
