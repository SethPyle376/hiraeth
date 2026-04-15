use std::collections::HashMap;

use hiraeth_auth::ResolvedRequest;
use hiraeth_core::{ServiceResponse, empty_response, json_response};
use hiraeth_store::sqs::SqsStore;
use serde::{Deserialize, Serialize};

use crate::{
    error::{SqsError, map_store_error},
    util,
};

const MAX_TAGS_PER_QUEUE: usize = 50;
const MAX_TAG_KEY_LENGTH: usize = 128;
const MAX_TAG_VALUE_LENGTH: usize = 256;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ListQueueTagsRequest {
    queue_url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
struct ListQueueTagsResponse {
    tags: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct TagQueueRequest {
    queue_url: String,
    tags: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct UntagQueueRequest {
    queue_url: String,
    tag_keys: Vec<String>,
}

pub(crate) async fn list_queue_tags<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
) -> Result<ServiceResponse, SqsError> {
    let request_body = util::parse_request_body::<ListQueueTagsRequest>(request)?;
    let queue = util::load_queue_from_url(request, store, &request_body.queue_url).await?;

    let tags = store
        .list_queue_tags(queue.id)
        .await
        .map_err(map_store_error)?;

    json_response(&ListQueueTagsResponse { tags }).map_err(Into::into)
}

pub(crate) async fn tag_queue<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
) -> Result<ServiceResponse, SqsError> {
    let request_body = util::parse_request_body::<TagQueueRequest>(request)?;
    validate_tags(&request_body.tags, false)?;

    let queue = util::load_queue_from_url(request, store, &request_body.queue_url).await?;
    let mut merged_tags = store
        .list_queue_tags(queue.id)
        .await
        .map_err(map_store_error)?;
    merged_tags.extend(request_body.tags.clone());
    validate_tags(&merged_tags, true)?;

    store
        .tag_queue(queue.id, request_body.tags)
        .await
        .map(|_| empty_response())
        .map_err(map_store_error)
}

pub(crate) async fn untag_queue<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
) -> Result<ServiceResponse, SqsError> {
    let request_body = util::parse_request_body::<UntagQueueRequest>(request)?;
    validate_tag_keys(&request_body.tag_keys, false)?;

    let queue = util::load_queue_from_url(request, store, &request_body.queue_url).await?;
    store
        .untag_queue(queue.id, request_body.tag_keys)
        .await
        .map(|_| empty_response())
        .map_err(map_store_error)
}

pub(crate) fn validate_tags(
    tags: &HashMap<String, String>,
    allow_empty: bool,
) -> Result<(), SqsError> {
    if !allow_empty && tags.is_empty() {
        return Err(SqsError::BadRequest(
            "Tags must contain at least one entry".to_string(),
        ));
    }

    if tags.len() > MAX_TAGS_PER_QUEUE {
        return Err(SqsError::BadRequest(format!(
            "A queue can have at most {MAX_TAGS_PER_QUEUE} tags"
        )));
    }

    for (key, value) in tags {
        validate_tag_key(key)?;
        if value.chars().count() > MAX_TAG_VALUE_LENGTH {
            return Err(SqsError::BadRequest(format!(
                "Tag value for '{}' must be at most {} characters",
                key, MAX_TAG_VALUE_LENGTH
            )));
        }
    }

    Ok(())
}

fn validate_tag_keys(tag_keys: &[String], allow_empty: bool) -> Result<(), SqsError> {
    if !allow_empty && tag_keys.is_empty() {
        return Err(SqsError::BadRequest(
            "TagKeys must contain at least one entry".to_string(),
        ));
    }

    if tag_keys.len() > MAX_TAGS_PER_QUEUE {
        return Err(SqsError::BadRequest(format!(
            "TagKeys can contain at most {MAX_TAGS_PER_QUEUE} entries"
        )));
    }

    for key in tag_keys {
        validate_tag_key(key)?;
    }

    Ok(())
}

fn validate_tag_key(key: &str) -> Result<(), SqsError> {
    let key_length = key.chars().count();

    if key_length == 0 || key_length > MAX_TAG_KEY_LENGTH {
        return Err(SqsError::BadRequest(format!(
            "Tag keys must be between 1 and {} characters",
            MAX_TAG_KEY_LENGTH
        )));
    }

    if key.starts_with("aws:") {
        return Err(SqsError::BadRequest(
            "Tag keys cannot start with the reserved aws: prefix".to_string(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_auth::{AuthContext, ResolvedRequest};
    use hiraeth_core::ServiceResponse;
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::{
        principal::Principal,
        sqs::{SqsQueue, SqsStore},
        test_support::SqsTestStore,
    };
    use serde_json::Value;

    use super::{list_queue_tags, tag_queue, untag_queue};
    use crate::error::SqsError;

    fn resolved_request(target: &str, body: &str) -> ResolvedRequest {
        ResolvedRequest {
            request: IncomingRequest {
                host: "localhost:4566".to_string(),
                method: "POST".to_string(),
                path: "/".to_string(),
                query: None,
                headers: [("x-amz-target".to_string(), target.to_string())]
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

    fn parse_json_body(response: &ServiceResponse) -> Value {
        serde_json::from_slice(&response.body).expect("response body should be valid json")
    }

    #[tokio::test]
    async fn list_queue_tags_returns_existing_tags() {
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
            "AmazonSQS.ListQueueTags",
            r#"{"QueueUrl":"http://localhost:4566/123456789012/orders"}"#,
        );

        let response = list_queue_tags(&request, &store)
            .await
            .expect("list queue tags should succeed");
        let body = parse_json_body(&response);

        assert_eq!(response.status_code, 200);
        assert_eq!(body["Tags"]["environment"], "test");
        assert_eq!(body["Tags"]["owner"], "hiraeth");
    }

    #[tokio::test]
    async fn tag_queue_merges_with_existing_tags() {
        let store = SqsTestStore::with_queue(queue());
        store
            .tag_queue(
                42,
                [("owner".to_string(), "old".to_string())]
                    .into_iter()
                    .collect(),
            )
            .await
            .expect("tags should seed");
        let request = resolved_request(
            "AmazonSQS.TagQueue",
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "Tags":{
                    "environment":"test",
                    "owner":"hiraeth"
                }
            }"#,
        );

        let response = tag_queue(&request, &store)
            .await
            .expect("tag queue should succeed");

        assert_eq!(response.status_code, 200);
        assert!(response.body.is_empty());
        assert_eq!(
            store.queue_tags(42),
            [
                ("environment".to_string(), "test".to_string()),
                ("owner".to_string(), "hiraeth".to_string()),
            ]
            .into_iter()
            .collect::<HashMap<_, _>>()
        );
    }

    #[tokio::test]
    async fn untag_queue_removes_requested_keys() {
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
            "AmazonSQS.UntagQueue",
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "TagKeys":["owner", "missing"]
            }"#,
        );

        let response = untag_queue(&request, &store)
            .await
            .expect("untag queue should succeed");

        assert_eq!(response.status_code, 200);
        assert!(response.body.is_empty());
        assert_eq!(
            store.queue_tags(42),
            [("environment".to_string(), "test".to_string())]
                .into_iter()
                .collect::<HashMap<_, _>>()
        );
    }

    #[tokio::test]
    async fn tag_queue_returns_not_found_for_missing_queue() {
        let store = SqsTestStore::default();
        let request = resolved_request(
            "AmazonSQS.TagQueue",
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "Tags":{"environment":"test"}
            }"#,
        );

        let result = tag_queue(&request, &store).await;

        assert!(matches!(result, Err(SqsError::QueueNotFound)));
    }

    #[tokio::test]
    async fn tag_queue_rejects_reserved_tag_key_prefix() {
        let store = SqsTestStore::with_queue(queue());
        let request = resolved_request(
            "AmazonSQS.TagQueue",
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "Tags":{"aws:reserved":"test"}
            }"#,
        );

        let result = tag_queue(&request, &store).await;

        assert!(matches!(result, Err(SqsError::BadRequest(_))));
        assert!(store.queue_tags(42).is_empty());
    }

    #[tokio::test]
    async fn tag_queue_rejects_more_than_fifty_total_tags() {
        let store = SqsTestStore::with_queue(queue());
        let existing_tags = (0..50)
            .map(|index| (format!("tag-{index}"), "value".to_string()))
            .collect();
        store
            .tag_queue(42, existing_tags)
            .await
            .expect("tags should seed");
        let request = resolved_request(
            "AmazonSQS.TagQueue",
            r#"{
                "QueueUrl":"http://localhost:4566/123456789012/orders",
                "Tags":{"one-more":"value"}
            }"#,
        );

        let result = tag_queue(&request, &store).await;

        assert!(matches!(result, Err(SqsError::BadRequest(_))));
        assert!(!store.queue_tags(42).contains_key("one-more"));
    }
}
