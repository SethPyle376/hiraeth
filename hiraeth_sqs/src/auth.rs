use hiraeth_core::ResolvedRequest;
use hiraeth_store::sqs::{SqsQueue, SqsStore};

use crate::{error::SqsError, queue::GetQueueUrlRequest, util};

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

pub(crate) fn get_action_for_request(request: &ResolvedRequest) -> Result<String, SqsError> {
    match request.request.headers.get("x-amz-target") {
        Some(value) => target_to_action(value),
        None => Err(SqsError::BadRequest(
            "Missing x-amz-target header".to_string(),
        )),
    }
}

fn target_to_action(target: &str) -> Result<String, SqsError> {
    let action = target
        .strip_prefix("AmazonSQS.")
        .ok_or_else(|| SqsError::UnsupportedOperation(target.to_string()))?;

    let action = match action {
        "CreateQueue" => "CreateQueue",
        "DeleteQueue" => "DeleteQueue",
        "GetQueueAttributes" => "GetQueueAttributes",
        "GetQueueUrl" => "GetQueueUrl",
        "ListQueueTags" => "ListQueueTags",
        "ListQueues" => "ListQueues",
        "PurgeQueue" => "PurgeQueue",
        "ReceiveMessage" => "ReceiveMessage",
        "SendMessage" => "SendMessage",
        "SetQueueAttributes" => "SetQueueAttributes",
        "TagQueue" => "TagQueue",
        "UntagQueue" => "UntagQueue",
        "ChangeMessageVisibility" => "ChangeMessageVisibility",
        "ChangeMessageVisibilityBatch" => "ChangeMessageVisibility",
        "DeleteMessage" => "DeleteMessage",
        "DeleteMessageBatch" => "DeleteMessage",
        "SendMessageBatch" => "SendMessage",
        _ => return Err(SqsError::UnsupportedOperation(target.to_string())),
    };

    Ok(format!("sqs:{action}"))
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

    use super::get_action_for_request;

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
    fn maps_sqs_target_to_iam_action() {
        let request = resolved_request(Some("AmazonSQS.ReceiveMessage"));

        let action = get_action_for_request(&request).expect("action should resolve");

        assert_eq!(action, "sqs:ReceiveMessage");
    }

    #[test]
    fn maps_batch_targets_to_underlying_iam_action() {
        for (target, expected_action) in [
            ("AmazonSQS.SendMessageBatch", "sqs:SendMessage"),
            ("AmazonSQS.DeleteMessageBatch", "sqs:DeleteMessage"),
            (
                "AmazonSQS.ChangeMessageVisibilityBatch",
                "sqs:ChangeMessageVisibility",
            ),
        ] {
            let request = resolved_request(Some(target));

            let action = get_action_for_request(&request).expect("action should resolve");

            assert_eq!(action, expected_action);
        }
    }

    #[test]
    fn rejects_unknown_sqs_target() {
        let request = resolved_request(Some("AmazonSQS.DoesNotExist"));

        let result = get_action_for_request(&request);

        assert!(
            matches!(result, Err(crate::error::SqsError::UnsupportedOperation(target)) if target == "AmazonSQS.DoesNotExist")
        );
    }
}
