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
    actions::util::{IAM_XMLNS, IamPolicyXml, parse_payload_error, parse_policy_arn},
    error::IamError,
};

pub(crate) struct GetPolicyAction;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct GetPolicyRequest {
    pub policy_arn: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct GetPolicyResponse {
    xmlns: &'static str,
    get_policy_result: GetPolicyResult,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct GetPolicyResult {
    policy: IamPolicyXml,
}

#[async_trait]
impl<S> TypedAwsAction<S> for GetPolicyAction
where
    S: IamStore + Send + Sync,
{
    type Request = GetPolicyRequest;
    type Response = GetPolicyResponse;
    type Error = IamError;

    fn name(&self) -> &'static str {
        "GetPolicy"
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
        get_request: Self::Request,
        store: &S,
        trace_context: &TraceContext,
        trace_recorder: &dyn TraceRecorder,
    ) -> Result<Self::Response, Self::Error> {
        let policy_arn = parse_policy_arn(&get_request.policy_arn)?;
        let attributes = HashMap::from([
            ("account_id".to_string(), policy_arn.account_id.clone()),
            ("policy_arn".to_string(), get_request.policy_arn.clone()),
            ("policy_name".to_string(), policy_arn.policy_name.clone()),
            ("policy_path".to_string(), policy_arn.policy_path.clone()),
        ]);

        let policy = trace_context
            .record_result_span(trace_recorder, "iam.policy.get", "iam", attributes, async {
                store
                    .get_managed_policy(
                        &policy_arn.account_id,
                        &policy_arn.policy_name,
                        &policy_arn.policy_path,
                    )
                    .await?
                    .ok_or_else(|| {
                        IamError::NoSuchEntity(format!(
                            "Policy {} does not exist",
                            get_request.policy_arn
                        ))
                    })
            })
            .await?;

        Ok(GetPolicyResponse {
            xmlns: IAM_XMLNS,
            get_policy_result: GetPolicyResult {
                policy: policy.into(),
            },
        })
    }

    async fn resolve_authorization(
        &self,
        request: &ResolvedRequest,
        get_request: Self::Request,
        store: &S,
    ) -> Result<AuthorizationCheck, Self::Error> {
        Ok(AuthorizationCheck {
            action: "iam:GetPolicy".to_string(),
            resource: get_request.policy_arn,
            resource_policy: None,
        })
    }
}
