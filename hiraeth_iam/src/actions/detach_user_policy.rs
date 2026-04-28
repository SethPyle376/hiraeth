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
        response_metadata,
    },
    error::IamError,
};

pub(crate) struct DetachUserPolicyAction;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct DetachUserPolicyRequest {
    pub user_name: String,
    pub policy_arn: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
struct DetachUserPolicyResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    response_metadata: ResponseMetadata,
}

#[async_trait]
impl<S> TypedAwsAction<S> for DetachUserPolicyAction
where
    S: IamStore + Send + Sync,
{
    type Request = DetachUserPolicyRequest;
    type Error = IamError;

    fn name(&self) -> &'static str {
        "DetachUserPolicy"
    }

    fn parse_error(&self, error: AwsActionPayloadParseError) -> Self::Error {
        parse_payload_error(error)
    }

    async fn handle_typed(
        &self,
        request: ResolvedRequest,
        detach_policy_request: DetachUserPolicyRequest,
        store: &S,
    ) -> Result<ServiceResponse, IamError> {
        let account_id = &request.auth_context.principal.account_id;
        let user = store
            .get_principal_by_identity(account_id, "user", &detach_policy_request.user_name)
            .await?
            .ok_or_else(|| {
                IamError::NoSuchEntity(format!("User {}", detach_policy_request.user_name))
            })?;

        let policy_arn = parse_policy_arn(&detach_policy_request.policy_arn)?;
        let policy = store
            .get_managed_policy(&policy_arn.0, &policy_arn.1)
            .await?
            .ok_or_else(|| {
                IamError::NoSuchEntity(format!(
                    "Policy {} does not exist",
                    detach_policy_request.policy_arn
                ))
            })?;
        store
            .detach_policy_from_principal(policy.id, user.id)
            .await?;

        let response = DetachUserPolicyResponse {
            xmlns: IAM_XMLNS,
            response_metadata: response_metadata(Uuid::new_v4().to_string()),
        };
        iam_xml_response(&response)
    }

    async fn resolve_authorization_typed(
        &self,
        request: &ResolvedRequest,
        attach_policy_request: DetachUserPolicyRequest,
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
            action: "iam:DetachUserPolicy".to_string(),
            resource: arn_util::user_arn(account_id, &user.path, &user.name),
            resource_policy: None,
        })
    }
}
