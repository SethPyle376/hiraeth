use async_trait::async_trait;
use hiraeth_core::{
    AwsActionPayloadParseError, ResolvedRequest, ServiceResponse, TypedAwsAction, arn_util,
    auth::AuthorizationCheck, xml_response,
};
use hiraeth_store::IamStore;
use serde::{Deserialize, Serialize};

use crate::{actions::util::parse_payload_error, error::StsError};

pub(crate) struct GetCallerIdentityAction;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct GetCallerIdentityRequest {}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
struct GetCallerIdentityResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    #[serde(rename = "GetCallerIdentityResult")]
    result: GetCallerIdentityResult,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
struct GetCallerIdentityResult {
    arn: String,
    user_id: String,
    account: String,
}

#[async_trait]
impl<S> TypedAwsAction<S> for GetCallerIdentityAction
where
    S: IamStore + Send + Sync,
{
    type Request = GetCallerIdentityRequest;
    type Error = StsError;

    fn name(&self) -> &'static str {
        "GetCallerIdentity"
    }

    fn parse_error(&self, error: AwsActionPayloadParseError) -> Self::Error {
        parse_payload_error(error)
    }

    async fn handle_typed(
        &self,
        request: ResolvedRequest,
        get_caller_identity_request: Self::Request,
        store: &S,
    ) -> Result<ServiceResponse, Self::Error> {
        let account_id = &request.auth_context.principal.account_id;
        let name = &request.auth_context.principal.name;

        let user = store
            .get_principal_by_identity(account_id, "user", name)
            .await?
            .ok_or_else(|| StsError::InternalError("User not found".to_string()))?;

        let response = GetCallerIdentityResponse {
            xmlns: "https://sts.amazonaws.com/doc/2011-06-15/",
            result: GetCallerIdentityResult {
                arn: arn_util::user_arn(account_id, &user.path, &user.name),
                user_id: user.user_id.clone(),
                account: account_id.clone(),
            },
        };

        Ok(xml_response(&response).map_err(StsError::from)?)
    }

    async fn resolve_authorization_typed(
        &self,
        request: &ResolvedRequest,
        get_caller_identity_request: GetCallerIdentityRequest,
        store: &S,
    ) -> Result<AuthorizationCheck, StsError> {
        Ok(AuthorizationCheck {
            action: "sts:GetCallerIdentity".to_string(),
            resource: format!(
                "arn:aws:iam::{}:user/{}",
                request.auth_context.principal.account_id, request.auth_context.principal.name
            ),
            resource_policy: None,
        })
    }
}
