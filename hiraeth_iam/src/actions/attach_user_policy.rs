use async_trait::async_trait;
use hiraeth_core::{
    AwsActionPayloadParseError, ResolvedRequest, ServiceResponse, TypedAwsAction, arn_util,
    auth::AuthorizationCheck,
};
use hiraeth_store::IamStore;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    actions::util::{
        IAM_XMLNS, ResponseMetadata, iam_xml_response, parse_payload_error, parse_policy_arn,
    },
    error::IamError,
};

pub(crate) struct AttachUserPolicyAction;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct AttachUserPolicyRequest {
    pub user_name: String,
    pub policy_arn: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
struct AttachUserPolicyResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    response_metadata: ResponseMetadata,
}

#[async_trait]
impl<S> TypedAwsAction<S> for AttachUserPolicyAction
where
    S: IamStore + Send + Sync,
{
    type Request = AttachUserPolicyRequest;
    type Error = IamError;

    fn name(&self) -> &'static str {
        "AttachUserPolicy"
    }

    fn parse_error(&self, error: AwsActionPayloadParseError) -> Self::Error {
        parse_payload_error(error)
    }

    async fn handle_typed(
        &self,
        request: ResolvedRequest,
        attach_policy_request: AttachUserPolicyRequest,
        store: &S,
    ) -> Result<ServiceResponse, IamError> {
        let account_id = &request.auth_context.principal.account_id;
        let user = store
            .get_principal_by_identity(account_id, "user", &attach_policy_request.user_name)
            .await?
            .ok_or_else(|| {
                IamError::NoSuchEntity(format!("User {}", attach_policy_request.user_name))
            })?;

        let arn = parse_policy_arn(&attach_policy_request.policy_arn)?;
        if arn.account_id != *account_id {
            return Err(IamError::NoSuchEntity(format!(
                "Policy {} does not exist",
                attach_policy_request.policy_arn
            )));
        }

        let policy = store
            .get_managed_policy(&arn.account_id, &arn.policy_name, &arn.policy_path)
            .await?
            .ok_or_else(|| {
                IamError::NoSuchEntity(format!(
                    "Policy {} does not exist",
                    attach_policy_request.policy_arn
                ))
            })?;

        store
            .attach_policy_to_principal(policy.id, user.id)
            .await
            .map(|_| {
                iam_xml_response(&AttachUserPolicyResponse {
                    xmlns: IAM_XMLNS,
                    response_metadata: ResponseMetadata {
                        request_id: Uuid::new_v4().to_string(),
                    },
                })
            })?
    }

    async fn resolve_authorization_typed(
        &self,
        request: &ResolvedRequest,
        attach_policy_request: AttachUserPolicyRequest,
        store: &S,
    ) -> Result<AuthorizationCheck, IamError> {
        let account_id = &request.auth_context.principal.account_id;
        let user = store
            .get_principal_by_identity(account_id, "user", &attach_policy_request.user_name)
            .await?
            .ok_or_else(|| {
                IamError::NoSuchEntity(format!("User {}", attach_policy_request.user_name))
            })?;

        Ok(AuthorizationCheck {
            action: "iam:AttachUserPolicy".to_string(),
            resource: arn_util::user_arn(account_id, &user.path, &user.name),
            resource_policy: None,
        })
    }
}
