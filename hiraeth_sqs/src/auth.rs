use hiraeth_core::{ResolvedRequest, ServiceResponse, auth::AuthorizationCheck, auth::Policy};
use hiraeth_store::sqs::{SqsQueue, SqsStore};

use crate::{actions::GetQueueUrlRequest, error::SqsError, util};

pub(crate) async fn get_relevant_queue_for_action<S: SqsStore>(
    action: &str,
    request: &ResolvedRequest,
    store: &S,
) -> Result<Option<SqsQueue>, SqsError> {
    match action {
        "sqs:ListQueues" => Ok(None),
        "sqs:CreateQueue" => Ok(None),
        "sqs:GetQueueUrl" => {
            let request_body = util::parse_request_body::<GetQueueUrlRequest>(request)?;
            let account_id = request_body
                .queue_owner_aws_account_id
                .unwrap_or_else(|| request.auth_context.principal.account_id.clone());

            store
                .get_queue(&request_body.queue_name, &request.region, &account_id)
                .await
                .map_err(|e| SqsError::InternalError(e.to_string()))
        }
        _ => Ok(Some(get_queue_from_request(request, store).await?)),
    }
}

pub(crate) fn get_action_name_for_request(request: &ResolvedRequest) -> Result<String, SqsError> {
    match request.request.headers.get("x-amz-target") {
        Some(value) => target_to_operation_name(value),
        None => Err(SqsError::BadRequest(
            "Missing x-amz-target header".to_string(),
        )),
    }
}

fn target_to_operation_name(target: &str) -> Result<String, SqsError> {
    let action = target
        .strip_prefix("AmazonSQS.")
        .ok_or_else(|| SqsError::UnsupportedOperation(target.to_string()))?;

    match action {
        "CreateQueue"
        | "DeleteQueue"
        | "GetQueueAttributes"
        | "GetQueueUrl"
        | "ListQueueTags"
        | "ListQueues"
        | "PurgeQueue"
        | "ReceiveMessage"
        | "SendMessage"
        | "SendMessageBatch"
        | "SetQueueAttributes"
        | "TagQueue"
        | "UntagQueue"
        | "ChangeMessageVisibility"
        | "ChangeMessageVisibilityBatch"
        | "DeleteMessage"
        | "DeleteMessageBatch" => Ok(action.to_string()),
        _ => Err(SqsError::UnsupportedOperation(target.to_string())),
    }
}

pub(crate) async fn resolve_authorization<S: SqsStore>(
    authorization_action: &str,
    request: &ResolvedRequest,
    store: &S,
) -> Result<AuthorizationCheck, ServiceResponse> {
    let relevant_queue = get_relevant_queue_for_action(authorization_action, request, store)
        .await
        .map_err(ServiceResponse::from)?;

    let resource = relevant_queue
        .as_ref()
        .map(util::get_queue_arn)
        .unwrap_or_else(|| {
            format!(
                "arn:aws:sqs:{}:{}:*",
                request.region, request.auth_context.principal.account_id
            )
        });

    let policy = relevant_queue
        .map(|queue| queue.policy.clone())
        .map(|policy| {
            serde_json::from_str::<Policy>(&policy).unwrap_or_else(|_| Policy::default())
        });

    Ok(AuthorizationCheck {
        action: authorization_action.to_string(),
        resource,
        resource_policy: policy,
    })
}

fn get_queue_url_from_request(request: &ResolvedRequest) -> Result<String, SqsError> {
    let payload = serde_json::from_slice::<serde_json::Value>(&request.request.body)
        .map_err(|e| SqsError::BadRequest(format!("Invalid JSON body: {}", e)))?;

    let queue_url = payload
        .get("QueueUrl")
        .and_then(|v| v.as_str())
        .ok_or_else(|| SqsError::BadRequest("Missing QueueUrl in request body".to_string()))?;

    Ok(queue_url.to_string())
}

async fn get_queue_from_request<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
) -> Result<SqsQueue, SqsError> {
    let queue_url = get_queue_url_from_request(request)?;
    let queue = util::load_queue_from_url(request, store, &queue_url).await?;
    Ok(queue)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_core::{AuthContext, ResolvedRequest};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::principal::Principal;

    use super::get_action_name_for_request;

    fn resolved_request(target: Option<&str>) -> ResolvedRequest {
        let mut headers = HashMap::new();
        if let Some(target) = target {
            headers.insert("x-amz-target".to_string(), target.to_string());
        }

        ResolvedRequest {
            request: IncomingRequest {
                host: "sqs.us-east-1.amazonaws.com".to_string(),
                method: "POST".to_string(),
                path: "/".to_string(),
                query: None,
                headers,
                body: b"{}".to_vec(),
            },
            service: "sqs".to_string(),
            region: "us-east-1".to_string(),
            auth_context: AuthContext {
                access_key: "AKIAIOSFODNN7EXAMPLE".to_string(),
                principal: Principal {
                    id: 1,
                    account_id: "123456789012".to_string(),
                    kind: "user".to_string(),
                    name: "test-user".to_string(),
                    path: "/".to_string(),
                    user_id: "AIDATESTUSER000001".to_string(),
                    created_at: Utc
                        .with_ymd_and_hms(2026, 4, 1, 12, 0, 0)
                        .unwrap()
                        .naive_utc(),
                },
            },
            date: Utc.with_ymd_and_hms(2026, 4, 1, 12, 0, 0).unwrap(),
        }
    }

    #[test]
    fn maps_sqs_target_to_operation_name() {
        let request = resolved_request(Some("AmazonSQS.ReceiveMessage"));

        let action = get_action_name_for_request(&request).expect("action should resolve");

        assert_eq!(action, "ReceiveMessage");
    }

    #[test]
    fn keeps_batch_targets_as_distinct_operations() {
        for target in [
            "AmazonSQS.SendMessageBatch",
            "AmazonSQS.DeleteMessageBatch",
            "AmazonSQS.ChangeMessageVisibilityBatch",
        ] {
            let request = resolved_request(Some(target));

            let action = get_action_name_for_request(&request).expect("action should resolve");

            assert_eq!(action, target.trim_start_matches("AmazonSQS."));
        }
    }

    #[test]
    fn rejects_unknown_sqs_target() {
        let request = resolved_request(Some("AmazonSQS.DoesNotExist"));

        let result = get_action_name_for_request(&request);

        assert!(
            matches!(result, Err(crate::error::SqsError::UnsupportedOperation(target)) if target == "AmazonSQS.DoesNotExist")
        );
    }
}
