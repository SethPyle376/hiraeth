use std::collections::HashMap;

use hiraeth_core::ResolvedRequest;
use hiraeth_store::sqs::SqsStore;
use serde::Deserialize;

use super::tag_support::validate_tag_keys;
use crate::error::SqsError;

pub(crate) struct UntagQueueAction;

hiraeth_core::impl_aws_action! {
    UntagQueueAction<S: SqsStore> {
        request: UntagQueueRequest,
        response: (),
        defaults: crate::SqsActionDefaults,
        name: "UntagQueue",
        response_format: Empty,
        validate: |_request, request_body, _store| {
            validate_tag_keys(&request_body.tag_keys, false)
        },
        handler: handle_untag_queue_typed,
        span: "sqs.queue.untag",
        span_attrs: |_request, payload, _store| {
            HashMap::from([
                ("queue_url".to_string(), payload.queue_url.clone()),
                (
                    "tag_key_count".to_string(),
                    payload.tag_keys.len().to_string(),
                ),
                ("tag_keys".to_string(), payload.tag_keys.join(",")),
            ])
        },
        authorize_action: "sqs:UntagQueue",
        authorize_with: crate::auth::resolve_authorization,
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct UntagQueueRequest {
    queue_url: String,
    tag_keys: Vec<String>,
}

async fn handle_untag_queue_typed<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
    request_body: UntagQueueRequest,
) -> Result<(), SqsError> {
    validate_tag_keys(&request_body.tag_keys, false)?;

    let queue = crate::util::load_queue_from_url(request, store, &request_body.queue_url).await?;
    store
        .untag_queue(queue.id, request_body.tag_keys)
        .await
        .map_err(crate::error::map_store_error)
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use hiraeth_core::{AuthContext, ResolvedRequest, TypedAwsAction};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::{
        principal::Principal, sqs::SqsQueue, sqs::SqsStore, test_support::SqsTestStore,
    };

    use super::{UntagQueueAction, handle_untag_queue_typed};

    fn resolved_request(body: &str) -> ResolvedRequest {
        ResolvedRequest {
            request_id: "test-request-id".to_string(),
            request: IncomingRequest {
                host: "localhost:4566".to_string(),
                method: "POST".to_string(),
                path: "/".to_string(),
                query: None,
                headers: [(
                    "x-amz-target".to_string(),
                    "AmazonSQS.UntagQueue".to_string(),
                )]
                .into_iter()
                .collect(),
                body: body.as_bytes().to_vec(),
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
                        .with_ymd_and_hms(2026, 4, 15, 12, 0, 0)
                        .unwrap()
                        .naive_utc(),
                },
            },
            date: Utc.with_ymd_and_hms(2026, 4, 15, 12, 0, 0).unwrap(),
        }
    }

    fn queue() -> SqsQueue {
        SqsQueue {
            id: 42,
            name: "orders".to_string(),
            region: "us-east-1".to_string(),
            account_id: "123456789012".to_string(),
            created_at: Utc
                .with_ymd_and_hms(2026, 4, 15, 12, 0, 0)
                .unwrap()
                .naive_utc(),
            updated_at: Utc
                .with_ymd_and_hms(2026, 4, 15, 12, 0, 0)
                .unwrap()
                .naive_utc(),
            ..Default::default()
        }
    }

    #[test]
    fn reports_expected_action_name() {
        assert_eq!(
            <UntagQueueAction as TypedAwsAction<SqsTestStore>>::name(&UntagQueueAction),
            "UntagQueue"
        );
    }

    #[tokio::test]
    async fn removes_requested_keys() {
        let store = SqsTestStore::with_queue(queue());
        store
            .tag_queue(
                42,
                [
                    ("environment".to_string(), "test".to_string()),
                    ("owner".to_string(), "hiraeth".to_string()),
                ]
                .into_iter()
                .collect(),
            )
            .await
            .expect("tags should seed");
        let request = resolved_request(
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "TagKeys":["owner"]
            }"#,
        );

        handle_untag_queue_typed(
            &request,
            &store,
            crate::actions::test_support::parse_request_body(&request),
        )
        .await
        .expect("untag queue should succeed");
        assert_eq!(
            store.queue_tags(42),
            [("environment".to_string(), "test".to_string())]
                .into_iter()
                .collect()
        );
    }
}
