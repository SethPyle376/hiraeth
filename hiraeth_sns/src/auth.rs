use hiraeth_core::{ResolvedRequest, auth::AuthorizationCheck, get_query_request_action_name};
use hiraeth_store::sns::SnsStore;

use crate::error::SnsError;

pub(crate) fn get_action_name_for_request(request: &ResolvedRequest) -> Result<String, SnsError> {
    let action_name = get_query_request_action_name(request)
        .map_err(|error| SnsError::BadRequest(error.to_string()))?
        .ok_or_else(|| SnsError::BadRequest("Missing Action parameter".to_string()))?;

    match action_name.as_str() {
        "CreateTopic"
        | "DeleteTopic"
        | "Subscribe"
        | "Unsubscribe"
        | "ListSubscriptionsByTopic"
        | "Publish" => Ok(action_name),
        _ => Err(SnsError::UnsupportedOperation(action_name)),
    }
}

pub(crate) async fn resolve_authorization<S: SnsStore>(
    authorization_action: &str,
    request: &ResolvedRequest,
    store: &S,
) -> Result<AuthorizationCheck, SnsError> {
    let topic_arn = extract_topic_arn_from_request(request).await?;

    let resource = topic_arn.clone().unwrap_or_else(|| {
        format!(
            "arn:aws:sns:{}:{}:*",
            request.region, request.auth_context.principal.account_id
        )
    });

    let policy = if let Some(ref arn) = topic_arn {
        store
            .get_topic(arn)
            .await?
            .map(|topic| topic.policy)
            .and_then(|policy| serde_json::from_str(&policy).ok())
    } else {
        None
    };

    Ok(AuthorizationCheck {
        action: authorization_action.to_string(),
        resource,
        resource_policy: policy,
    })
}

async fn extract_topic_arn_from_request(
    request: &ResolvedRequest,
) -> Result<Option<String>, SnsError> {
    let params = hiraeth_core::parse_aws_query_params(&request.request)
        .map_err(|e| SnsError::BadRequest(e.to_string()))?;

    Ok(params.get("TopicArn").map(String::from))
}
