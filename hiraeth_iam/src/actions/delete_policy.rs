use std::collections::HashMap;

use hiraeth_core::impl_aws_action;
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

impl_aws_action! {
    DeletePolicyAction<S: IamStore> {
        request: DeletePolicyRequest,
        response: DeletePolicyResponse,
        error: IamError,
        name: "DeletePolicy",
        payload: AwsQuery,
        response_format: Xml,
        parse_error: parse_payload_error,
        validate: |_request, delete_request, _store| {
            util::parse_policy_arn(&delete_request.policy_arn)?;
            Ok(())
        },
        handle: |request, delete_request, store, trace_context, trace_recorder| {
            let policy_arn = util::parse_policy_arn(&delete_request.policy_arn)?;
            if policy_arn.account_id != request.auth_context.principal.account_id {
                return Err(IamError::NoSuchEntity(format!(
                    "Policy {} does not exist",
                    delete_request.policy_arn
                )));
            }
            let attributes = HashMap::from([
                ("account_id".to_string(), policy_arn.account_id.clone()),
                ("policy_arn".to_string(), delete_request.policy_arn.clone()),
                ("policy_name".to_string(), policy_arn.policy_name.clone()),
                ("policy_path".to_string(), policy_arn.policy_path.clone()),
            ]);
            trace_context
                .record_result_span(
                    trace_recorder,
                    "iam.policy.delete",
                    "iam",
                    attributes,
                    async {
                        store
                            .delete_managed_policy(
                                &policy_arn.account_id,
                                &policy_arn.policy_name,
                                &policy_arn.policy_path,
                            )
                            .await
                    },
                )
                .await?;

            Ok(DeletePolicyResponse {
                xmlns: IAM_XMLNS,
                response_metadata: response_metadata(request.request_id),
            })
        },
        authorize: |request, _payload, store| {
            crate::auth::resolve_authorization("iam:DeletePolicy", request, store).await
        },
    }
}
