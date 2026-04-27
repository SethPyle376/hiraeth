use async_trait::async_trait;
use hiraeth_core::{
    AwsActionPayloadParseError, ResolvedRequest, ServiceResponse, TypedAwsAction,
    auth::{AuthorizationCheck, Policy},
};
use hiraeth_store::{IamStore, iam::NewManagedPolicy};
use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    actions::util::{iam_xml_response, parse_payload_error},
    error::IamError,
};

pub(crate) struct CreatePolicyAction;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct CreatePolicyRequest {
    path: Option<String>,
    policy_document: String,
    policy_name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
struct CreatePolicyResponse {}

#[async_trait]
impl<S> TypedAwsAction<S> for CreatePolicyAction
where
    S: IamStore + Send + Sync,
{
    type Request = CreatePolicyRequest;
    type Error = IamError;

    fn name(&self) -> &'static str {
        "CreatePolicy"
    }

    fn parse_error(&self, error: AwsActionPayloadParseError) -> IamError {
        parse_payload_error(error)
    }

    async fn handle_typed(
        &self,
        request: ResolvedRequest,
        create_policy_request: CreatePolicyRequest,
        store: &S,
    ) -> Result<ServiceResponse, IamError> {
        let account_id = request.auth_context.principal.account_id.clone();
        let created_policy = store
            .insert_managed_policy(NewManagedPolicy {
                account_id,
                policy_name: create_policy_request.policy_name.clone(),
                policy_path: create_policy_request.path.clone(),
                policy_document: create_policy_request.policy_document.clone(),
            })
            .await?;

        iam_xml_response(&CreatePolicyResponse {})
    }

    async fn resolve_authorization_typed(
        &self,
        request: &ResolvedRequest,
        create_policy_request: CreatePolicyRequest,
        _store: &S,
    ) -> Result<AuthorizationCheck, IamError> {
        Ok(AuthorizationCheck {
            action: "iam:CreatePolicy".to_string(),
            resource: format!(
                "arn:aws:iam::{}:policy/{}",
                request.auth_context.principal.account_id, create_policy_request.policy_name
            ),
            resource_policy: None,
        })
    }
}
