use std::collections::HashMap;

use async_trait::async_trait;
use hiraeth_core::{
    AwsActionPayloadParseError, AwsActionResponseFormat, ResolvedRequest, TypedAwsAction,
    arn_util::user_arn,
    auth::AuthorizationCheck,
    tracing::{TraceContext, TraceRecorder},
};
use hiraeth_store::IamStore;
use serde::{Deserialize, Serialize};

use crate::{
    actions::util::{IAM_XMLNS, ResponseMetadata, parse_payload_error},
    error::IamError,
};

pub(crate) struct GetUserPolicyAction;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct GetUserPolicyRequest {
    user_name: String,
    policy_name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct GetUserPolicyResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    get_user_policy_result: GetUserPolicyResult,
    response_metadata: ResponseMetadata,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
struct GetUserPolicyResult {
    user_name: String,
    policy_name: String,
    policy_document: String,
}

#[async_trait]
impl<S> TypedAwsAction<S> for GetUserPolicyAction
where
    S: IamStore + Send + Sync,
{
    type Request = GetUserPolicyRequest;
    type Response = GetUserPolicyResponse;
    type Error = IamError;

    fn name(&self) -> &'static str {
        "GetUserPolicy"
    }

    fn parse_error(&self, error: AwsActionPayloadParseError) -> Self::Error {
        parse_payload_error(error)
    }

    fn response_format(&self) -> AwsActionResponseFormat {
        AwsActionResponseFormat::Xml
    }

    async fn handle(
        &self,
        request: ResolvedRequest,
        get_user_policy_request: Self::Request,
        store: &S,
        trace_context: &TraceContext,
        trace_recorder: &dyn TraceRecorder,
    ) -> Result<Self::Response, Self::Error> {
        let account_id = &request.auth_context.principal.account_id;
        let attributes = HashMap::from([
            (
                "user_name".to_string(),
                get_user_policy_request.user_name.clone(),
            ),
            (
                "policy_name".to_string(),
                get_user_policy_request.policy_name.clone(),
            ),
        ]);

        let (principal, policy) = trace_context
            .record_result_span(
                trace_recorder,
                "iam.get_user_policy",
                "iam",
                attributes,
                async {
                    let principal = store
                        .get_principal_by_identity(
                            account_id,
                            "user",
                            get_user_policy_request.user_name.as_str(),
                        )
                        .await?
                        .ok_or_else(|| {
                            IamError::NoSuchEntity(format!(
                                "User {} does not exist",
                                get_user_policy_request.user_name
                            ))
                        })?;

                    let policy = store
                        .get_principal_policy(principal.id, &get_user_policy_request.policy_name)
                        .await?
                        .ok_or_else(|| {
                            IamError::NoSuchEntity(format!(
                                "Policy {} does not exist for user {}",
                                get_user_policy_request.policy_name,
                                get_user_policy_request.user_name
                            ))
                        })?;

                    Ok::<_, IamError>((principal, policy))
                },
            )
            .await?;

        Ok(GetUserPolicyResponse {
            xmlns: IAM_XMLNS,
            get_user_policy_result: GetUserPolicyResult {
                user_name: principal.name,
                policy_name: policy.policy_name,
                policy_document: policy.policy_document,
            },
            response_metadata: ResponseMetadata {
                request_id: request.request_id,
            },
        })
    }

    async fn resolve_authorization_typed(
        &self,
        request: &ResolvedRequest,
        get_user_policy_request: Self::Request,
        store: &S,
    ) -> Result<AuthorizationCheck, Self::Error> {
        let account_id = &request.auth_context.principal.account_id;
        let user = store
            .get_principal_by_identity(
                account_id,
                "user",
                get_user_policy_request.user_name.as_str(),
            )
            .await?
            .ok_or_else(|| {
                IamError::NoSuchEntity(format!(
                    "User {} does not exist",
                    get_user_policy_request.user_name
                ))
            })?;
        let user_arn = user_arn(account_id, &user.path, &user.name);

        Ok(AuthorizationCheck {
            action: "iam:GetUserPolicy".to_string(),
            resource: user_arn,
            resource_policy: None,
        })
    }
}
