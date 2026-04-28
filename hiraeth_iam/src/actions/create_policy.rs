use async_trait::async_trait;
use hiraeth_core::{
    AwsActionPayloadParseError, ResolvedRequest, ServiceResponse, TypedAwsAction,
    auth::{AuthorizationCheck, Policy},
};
use hiraeth_store::{IamStore, iam::NewManagedPolicy};
use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

use crate::{
    actions::util::{
        IAM_XMLNS, IamPolicyXml, ResponseMetadata, iam_xml_response, new_id, normalize_policy_path,
        parse_payload_error, response_metadata,
    },
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
struct CreatePolicyResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    create_policy_result: CreatePolicyResult,
    response_metadata: ResponseMetadata,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
struct CreatePolicyResult {
    policy: IamPolicyXml,
}

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
        let policy_path = normalize_policy_path(create_policy_request.path.as_deref());
        let created_policy = store
            .insert_managed_policy(NewManagedPolicy {
                account_id,
                policy_id: new_id(),
                policy_name: create_policy_request.policy_name.clone(),
                policy_path: Some(policy_path),
                policy_document: create_policy_request.policy_document.clone(),
            })
            .await?;

        iam_xml_response(&CreatePolicyResponse {
            xmlns: IAM_XMLNS,
            create_policy_result: CreatePolicyResult {
                policy: created_policy.into(),
            },
            response_metadata: response_metadata(Uuid::new_v4().to_string()),
        })
    }

    async fn resolve_authorization_typed(
        &self,
        request: &ResolvedRequest,
        create_policy_request: CreatePolicyRequest,
        _store: &S,
    ) -> Result<AuthorizationCheck, IamError> {
        let policy_path = normalize_policy_path(create_policy_request.path.as_deref());
        Ok(AuthorizationCheck {
            action: "iam:CreatePolicy".to_string(),
            resource: format!(
                "arn:aws:iam::{}:policy{}{}",
                request.auth_context.principal.account_id,
                policy_path,
                create_policy_request.policy_name
            ),
            resource_policy: None,
        })
    }
}
