use std::collections::HashMap;

use hiraeth_core::ResolvedRequest;
use hiraeth_core::{ServiceResponse, empty_response, json_response};
use hiraeth_store::StoreError;
use hiraeth_store::sqs::{SqsQueue, SqsStore};
use serde::{Deserialize, Serialize};

use crate::{
    error::{SqsError, map_store_error},
    queue_attributes::QueueAttributeValues,
    tags::validate_tags,
    util,
};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CreateQueueRequest {
    queue_name: String,
    #[serde(default)]
    attributes: HashMap<String, String>,
    #[serde(default)]
    tags: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
struct CreateQueueResponse {
    queue_url: String,
}

pub(crate) async fn create_queue<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
) -> Result<ServiceResponse, SqsError> {
    let request_body = util::parse_request_body::<CreateQueueRequest>(request)?;
    let queue_attributes = QueueAttributeValues::from_attribute_map(&request_body.attributes)?;
    validate_queue_name(&request_body.queue_name, queue_attributes.fifo_queue)?;
    validate_tags(&request_body.tags, true)?;

    let now = chrono::Utc::now().naive_utc();
    let queue = SqsQueue {
        id: 0,
        name: request_body.queue_name.clone(),
        region: request.region.clone(),
        account_id: request.auth_context.principal.account_id.clone(),
        queue_type: if queue_attributes.fifo_queue {
            "fifo".to_string()
        } else {
            "standard".to_string()
        },
        visibility_timeout_seconds: queue_attributes.visibility_timeout_seconds,
        delay_seconds: queue_attributes.delay_seconds,
        maximum_message_size: queue_attributes.maximum_message_size,
        message_retention_period_seconds: queue_attributes.message_retention_period_seconds,
        receive_message_wait_time_seconds: queue_attributes.receive_message_wait_time_seconds,
        policy: queue_attributes.policy,
        redrive_policy: queue_attributes.redrive_policy,
        content_based_deduplication: queue_attributes.content_based_deduplication,
        kms_master_key_id: queue_attributes.kms_master_key_id,
        kms_data_key_reuse_period_seconds: queue_attributes.kms_data_key_reuse_period_seconds,
        deduplication_scope: queue_attributes.deduplication_scope,
        fifo_throughput_limit: queue_attributes.fifo_throughput_limit,
        redrive_allow_policy: queue_attributes.redrive_allow_policy,
        sqs_managed_sse_enabled: queue_attributes.sqs_managed_sse_enabled,
        created_at: now,
        updated_at: now,
    };

    match store.create_queue(queue.clone()).await {
        Ok(()) => {
            if !request_body.tags.is_empty() {
                let created_queue = store
                    .get_queue(
                        &request_body.queue_name,
                        &request.region,
                        &request.auth_context.principal.account_id,
                    )
                    .await
                    .map_err(|e| SqsError::InternalError(e.to_string()))?
                    .ok_or_else(|| {
                        SqsError::InternalError(
                            "created queue could not be loaded for tagging".to_string(),
                        )
                    })?;

                store
                    .tag_queue(created_queue.id, request_body.tags)
                    .await
                    .map_err(map_store_error)?;
            }

            create_queue_response(
                request,
                &request.auth_context.principal.account_id,
                &request_body.queue_name,
            )
        }
        Err(StoreError::Conflict(_)) => {
            let existing_queue = store
                .get_queue(
                    &request_body.queue_name,
                    &request.region,
                    &request.auth_context.principal.account_id,
                )
                .await
                .map_err(|e| SqsError::InternalError(e.to_string()))?;

            match existing_queue {
                Some(existing_queue) if queue_configuration_matches(&existing_queue, &queue) => {
                    create_queue_response(
                        request,
                        &request.auth_context.principal.account_id,
                        &request_body.queue_name,
                    )
                }
                Some(_) => Err(SqsError::QueueAlreadyExists(format!(
                    "A queue named '{}' already exists with different attributes.",
                    request_body.queue_name
                ))),
                None => Err(SqsError::InternalError(
                    "queue creation conflicted but existing queue could not be loaded".to_string(),
                )),
            }
        }
        Err(e) => Err(SqsError::InternalError(e.to_string())),
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DeleteQueueRequest {
    queue_url: String,
}

pub(crate) async fn delete_queue<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
) -> Result<ServiceResponse, SqsError> {
    let request_body = util::parse_request_body::<DeleteQueueRequest>(request)?;
    let queue = util::load_queue_from_url(request, store, &request_body.queue_url).await?;

    store
        .delete_queue(queue.id)
        .await
        .map(|_| empty_response())
        .map_err(map_store_error)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct GetQueueUrlRequest {
    pub queue_name: String,
    pub queue_owner_aws_account_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
struct GetQueueUrlResponse {
    queue_url: String,
}

pub(crate) async fn get_queue_url<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
) -> Result<ServiceResponse, SqsError> {
    let request_body = util::parse_request_body::<GetQueueUrlRequest>(request)?;
    validate_queue_name(
        &request_body.queue_name,
        request_body.queue_name.ends_with(".fifo"),
    )?;

    let account_id = request_body
        .queue_owner_aws_account_id
        .unwrap_or_else(|| request.auth_context.principal.account_id.clone());

    let queue = store
        .get_queue(&request_body.queue_name, &request.region, &account_id)
        .await
        .map_err(|e| SqsError::InternalError(e.to_string()))?;

    match queue {
        Some(_) => {
            let response = GetQueueUrlResponse {
                queue_url: util::queue_url(
                    &request.request.host,
                    &account_id,
                    &request_body.queue_name,
                ),
            };
            json_response(&response).map_err(Into::into)
        }
        None => Err(SqsError::QueueNotFound),
    }
}

fn create_queue_response(
    request: &ResolvedRequest,
    account_id: &str,
    queue_name: &str,
) -> Result<ServiceResponse, SqsError> {
    let response = CreateQueueResponse {
        queue_url: util::queue_url(&request.request.host, account_id, queue_name),
    };
    json_response(&response).map_err(Into::into)
}

fn validate_queue_name(queue_name: &str, fifo_queue: bool) -> Result<(), SqsError> {
    if queue_name.is_empty() || queue_name.len() > 80 {
        return Err(SqsError::BadRequest(
            "QueueName must be between 1 and 80 characters".to_string(),
        ));
    }

    let name_without_fifo_suffix = queue_name.strip_suffix(".fifo").unwrap_or(queue_name);
    let valid_chars = name_without_fifo_suffix
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_');
    if !valid_chars {
        return Err(SqsError::BadRequest(
            "QueueName may only contain alphanumeric characters, hyphens, and underscores"
                .to_string(),
        ));
    }

    if fifo_queue && !queue_name.ends_with(".fifo") {
        return Err(SqsError::BadRequest(
            "FIFO queue names must end with .fifo".to_string(),
        ));
    }

    if !fifo_queue && queue_name.ends_with(".fifo") {
        return Err(SqsError::BadRequest(
            "Queue names ending with .fifo must set FifoQueue=true".to_string(),
        ));
    }

    Ok(())
}

fn queue_configuration_matches(existing: &SqsQueue, requested: &SqsQueue) -> bool {
    existing.queue_type == requested.queue_type
        && existing.visibility_timeout_seconds == requested.visibility_timeout_seconds
        && existing.delay_seconds == requested.delay_seconds
        && existing.maximum_message_size == requested.maximum_message_size
        && existing.message_retention_period_seconds == requested.message_retention_period_seconds
        && existing.receive_message_wait_time_seconds == requested.receive_message_wait_time_seconds
        && existing.policy == requested.policy
        && existing.redrive_policy == requested.redrive_policy
        && existing.content_based_deduplication == requested.content_based_deduplication
        && existing.kms_master_key_id == requested.kms_master_key_id
        && existing.kms_data_key_reuse_period_seconds == requested.kms_data_key_reuse_period_seconds
        && existing.deduplication_scope == requested.deduplication_scope
        && existing.fifo_throughput_limit == requested.fifo_throughput_limit
        && existing.redrive_allow_policy == requested.redrive_allow_policy
        && existing.sqs_managed_sse_enabled == requested.sqs_managed_sse_enabled
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PurgeQueueRequest {
    queue_url: String,
}

pub(crate) async fn purge_queue<S: SqsStore>(
    request: &ResolvedRequest,
    store: &S,
) -> Result<ServiceResponse, SqsError> {
    let request_body = util::parse_request_body::<PurgeQueueRequest>(request)?;
    let queue = util::load_queue_from_url(request, store, &request_body.queue_url).await?;

    store
        .purge_queue(queue.id)
        .await
        .map(|_| empty_response())
        .map_err(map_store_error)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_core::{AuthContext, ResolvedRequest};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::{principal::Principal, sqs::SqsQueue, test_support::SqsTestStore};

    use super::{create_queue, delete_queue, purge_queue};
    use crate::error::SqsError;

    fn resolved_request(target: &str, body: &str) -> ResolvedRequest {
        let mut headers = HashMap::new();
        headers.insert("x-amz-target".to_string(), target.to_string());

        ResolvedRequest {
            request: IncomingRequest {
                host: "localhost:4566".to_string(),
                method: "POST".to_string(),
                path: "/".to_string(),
                query: None,
                headers,
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
                        .with_ymd_and_hms(2026, 4, 4, 12, 0, 0)
                        .unwrap()
                        .naive_utc(),
                },
            },
            date: Utc.with_ymd_and_hms(2026, 4, 4, 12, 0, 0).unwrap(),
        }
    }

    fn queue() -> SqsQueue {
        SqsQueue {
            id: 42,
            name: "orders".to_string(),
            region: "us-east-1".to_string(),
            account_id: "123456789012".to_string(),
            queue_type: "standard".to_string(),
            visibility_timeout_seconds: 30,
            delay_seconds: 0,
            message_retention_period_seconds: 345600,
            receive_message_wait_time_seconds: 0,
            created_at: Utc
                .with_ymd_and_hms(2026, 4, 4, 11, 0, 0)
                .unwrap()
                .naive_utc(),
            updated_at: Utc
                .with_ymd_and_hms(2026, 4, 4, 11, 0, 0)
                .unwrap()
                .naive_utc(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn delete_queue_deletes_existing_queue() {
        let store = SqsTestStore::with_queue(queue());
        let request = resolved_request(
            "AmazonSQS.DeleteQueue",
            r#"{"QueueUrl":"http://localhost:4566/123456789012/orders"}"#,
        );

        let response = delete_queue(&request, &store)
            .await
            .expect("delete queue should succeed");

        assert_eq!(response.status_code, 200);
        assert!(response.body.is_empty());

        let deleted = store.deleted_queue_ids();
        assert_eq!(deleted, vec![42]);
    }

    #[tokio::test]
    async fn create_queue_persists_supplied_tags() {
        let store = SqsTestStore::default();
        let request = resolved_request(
            "AmazonSQS.CreateQueue",
            r#"{
                "QueueName":"orders",
                "Tags":{
                    "environment":"test",
                    "owner":"hiraeth"
                }
            }"#,
        );

        let response = create_queue(&request, &store)
            .await
            .expect("create queue should succeed");

        assert_eq!(response.status_code, 200);
        assert_eq!(
            store.queue_tags(0),
            [
                ("environment".to_string(), "test".to_string()),
                ("owner".to_string(), "hiraeth".to_string()),
            ]
            .into_iter()
            .collect::<HashMap<_, _>>()
        );
    }

    #[tokio::test]
    async fn delete_queue_returns_not_found_for_missing_queue() {
        let store = SqsTestStore::default();
        let request = resolved_request(
            "AmazonSQS.DeleteQueue",
            r#"{"QueueUrl":"http://localhost:4566/123456789012/orders"}"#,
        );

        let result = delete_queue(&request, &store).await;

        assert!(matches!(result, Err(SqsError::QueueNotFound)));
    }

    #[tokio::test]
    async fn purge_queue_purges_existing_queue() {
        let store = SqsTestStore::with_queue(queue());
        let request = resolved_request(
            "AmazonSQS.PurgeQueue",
            r#"{"QueueUrl":"http://localhost:4566/123456789012/orders"}"#,
        );

        let response = purge_queue(&request, &store)
            .await
            .expect("purge queue should succeed");

        assert_eq!(response.status_code, 200);
        assert!(response.body.is_empty());
        assert_eq!(store.purged_queue_ids(), vec![42]);
    }

    #[tokio::test]
    async fn purge_queue_returns_not_found_for_missing_queue() {
        let store = SqsTestStore::default();
        let request = resolved_request(
            "AmazonSQS.PurgeQueue",
            r#"{"QueueUrl":"http://localhost:4566/123456789012/orders"}"#,
        );

        let result = purge_queue(&request, &store).await;

        assert!(matches!(result, Err(SqsError::QueueNotFound)));
        assert!(store.purged_queue_ids().is_empty());
    }
}
