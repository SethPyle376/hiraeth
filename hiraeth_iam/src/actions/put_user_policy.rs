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
        self, ResponseMetadata, iam_xml_response, parse_payload_error, response_metadata,
    },
    error::IamError,
};

pub(crate) struct PutUserPolicyAction;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct PutUserPolicyRequest {
    pub user_name: String,
    pub policy_name: String,
    pub policy_document: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
struct PutUserPolicyResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    response_metadata: ResponseMetadata,
}

#[async_trait]
impl<S> TypedAwsAction<S> for PutUserPolicyAction
where
    S: IamStore + Send + Sync,
{
    type Request = PutUserPolicyRequest;
    type Error = IamError;

    fn name(&self) -> &'static str {
        "PutUserPolicy"
    }

    fn parse_error(&self, error: AwsActionPayloadParseError) -> Self::Error {
        parse_payload_error(error)
    }

    async fn handle_typed(
        &self,
        request: ResolvedRequest,
        put_policy_request: Self::Request,
        store: &S,
    ) -> Result<ServiceResponse, Self::Error> {
        let account_id = &request.auth_context.principal.account_id;
        let user = store
            .get_principal_by_identity(&account_id, "user", &put_policy_request.user_name)
            .await?
            .ok_or_else(|| {
                IamError::NoSuchEntity(format!(
                    "User {} does not exist",
                    put_policy_request.user_name
                ))
            })?;

        store
            .put_inline_policy(
                user.id,
                &put_policy_request.policy_name,
                &put_policy_request.policy_document,
            )
            .await?;

        let response = PutUserPolicyResponse {
            xmlns: util::IAM_XMLNS,
            response_metadata: response_metadata(Uuid::new_v4().to_string()),
        };

        iam_xml_response(&response)
    }

    async fn resolve_authorization_typed(
        &self,
        request: &ResolvedRequest,
        put_policy_request: PutUserPolicyRequest,
        store: &S,
    ) -> Result<AuthorizationCheck, Self::Error> {
        let account_id = &request.auth_context.principal.account_id;
        let user = store
            .get_principal_by_identity(&account_id, "user", &put_policy_request.user_name)
            .await?
            .ok_or_else(|| {
                IamError::NoSuchEntity(format!(
                    "User {} does not exist",
                    put_policy_request.user_name
                ))
            })?;

        let arn = arn_util::user_arn(account_id, &user.path, &user.name);

        Ok(AuthorizationCheck {
            action: "iam:PutUserPolicy".to_string(),
            resource: arn,
            resource_policy: None,
        })
    }
}
