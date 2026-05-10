use std::collections::HashMap;

use hiraeth_core::{
    ResolvedRequest,
    tracing::{TraceContext, TraceRecorder},
};
use hiraeth_store::IamStore;
use serde::{Deserialize, Serialize};

use crate::{
    actions::util::{self, IAM_XMLNS, ResponseMetadata, response_metadata},
    error::IamError,
};

pub(crate) struct DeletePolicyAction;

crate::impl_iam_action! {
    DeletePolicyAction<S: IamStore> {
        request: DeletePolicyRequest,
        response: DeletePolicyResponse,
        name: "DeletePolicy",
        validate: |_request, delete_request, _store| {
            util::parse_policy_arn(&delete_request.policy_arn)?;
            Ok(())
        },
        handler: handle_delete_policy,
        authorize_action: "iam:DeletePolicy",
    }
}

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

async fn handle_delete_policy<S: IamStore + Send + Sync>(
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
}
