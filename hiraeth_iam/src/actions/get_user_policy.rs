use std::collections::HashMap;

use hiraeth_core::{
    ResolvedRequest,
    tracing::{TraceContext, TraceRecorder},
};
use hiraeth_store::IamStore;
use serde::{Deserialize, Serialize};

use crate::{
    actions::util::{IAM_XMLNS, ResponseMetadata},
    error::IamError,
};

pub(crate) struct GetUserPolicyAction;

hiraeth_core::impl_aws_action! {
    GetUserPolicyAction<S: IamStore> {
        request: GetUserPolicyRequest,
        response: GetUserPolicyResponse,
        defaults: crate::IamActionDefaults,
        name: "GetUserPolicy",
        handler: handle_get_user_policy,
        authorize_action: "iam:GetUserPolicy",
        authorize_with: crate::auth::resolve_authorization,
    }
}

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

async fn handle_get_user_policy<S: IamStore + Send + Sync>(
    request: ResolvedRequest,
    get_user_policy_request: GetUserPolicyRequest,
    store: &S,
    trace_context: &TraceContext,
    trace_recorder: &dyn TraceRecorder,
) -> Result<GetUserPolicyResponse, IamError> {
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
                            get_user_policy_request.policy_name, get_user_policy_request.user_name
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
