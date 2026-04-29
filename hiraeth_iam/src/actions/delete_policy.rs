use std::collections::HashMap;

use async_trait::async_trait;
use hiraeth_core::{
    AwsActionPayloadParseError, AwsActionResponseFormat, ResolvedRequest, TypedAwsAction,
    auth::AuthorizationCheck,
    tracing::{TraceContext, TraceRecorder},
};
use hiraeth_store::IamStore;
use serde::{Deserialize, Serialize};

use crate::{
    actions::util::{self, IAM_XMLNS, ResponseMetadata, parse_payload_error, response_metadata},
    error::IamError,
};

pub(crate) struct DeletePolicyAction;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct DeletePolicyRequest {
    pub policy_arn: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct DeletePolicyResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    response_metadata: ResponseMetadata,
}

#[async_trait]
impl<S> TypedAwsAction<S> for DeletePolicyAction
where
    S: IamStore + Send + Sync,
{
    type Request = DeletePolicyRequest;
    type Response = DeletePolicyResponse;
    type Error = IamError;

    fn name(&self) -> &'static str {
        "DeletePolicy"
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
        delete_request: DeletePolicyRequest,
        store: &S,
        trace_context: &TraceContext,
        trace_recorder: &dyn TraceRecorder,
    ) -> Result<DeletePolicyResponse, IamError> {
        let policy_arn = util::parse_policy_arn(&delete_request.policy_arn)?;
        if policy_arn.account_id != request.auth_context.principal.account_id {
            return Err(IamError::NoSuchEntity(format!(
                "Policy {} does not exist",
                delete_request.policy_arn
            )));
        }
        let timer = trace_context.start_span();
        let attributes = HashMap::from([
            ("account_id".to_string(), policy_arn.account_id.clone()),
            ("policy_arn".to_string(), delete_request.policy_arn.clone()),
            ("policy_name".to_string(), policy_arn.policy_name.clone()),
            ("policy_path".to_string(), policy_arn.policy_path.clone()),
        ]);
        let result = store
            .delete_managed_policy(
                &policy_arn.account_id,
                &policy_arn.policy_name,
                &policy_arn.policy_path,
            )
            .await;
        let status = if result.is_ok() { "ok" } else { "error" };
        trace_context
            .record_span_or_warn(
                trace_recorder,
                timer,
                "iam.policy.delete",
                "iam",
                status,
                attributes,
            )
            .await;
        result?;

        Ok(DeletePolicyResponse {
            xmlns: IAM_XMLNS,
            response_metadata: response_metadata(request.request_id),
        })
    }

    async fn resolve_authorization_typed(
        &self,
        request: &ResolvedRequest,
        delete_policy_request: DeletePolicyRequest,
        store: &S,
    ) -> Result<AuthorizationCheck, IamError> {
        Ok(AuthorizationCheck {
            action: "iam:DeletePolicy".to_string(),
            resource: delete_policy_request.policy_arn,
            resource_policy: None,
        })
    }
}
