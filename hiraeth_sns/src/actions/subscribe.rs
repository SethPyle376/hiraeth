use std::collections::HashMap;

use chrono::Utc;
use hiraeth_core::ResolvedRequest;
use hiraeth_store::sns::{SnsStore, SnsSubscription};
use serde::{Deserialize, Serialize};

use super::action_support::{
    SnsAttributes, is_valid_subscription_attribute, validate_json_attribute,
    validate_raw_message_delivery,
};
use crate::{
    actions::action_support::{ResponseMetadata, SNS_XMLNS, validate_topic_arn},
    error::SnsError,
};

pub(crate) struct SubscribeAction;

hiraeth_core::impl_aws_action! {
    SubscribeAction<S: SnsStore> {
        request: SubscribeRequest,
        response: SubscribeResponse,
        defaults: crate::SnsActionDefaults,
        name: "Subscribe",
        validate: |_request, payload, _store| {
            validate_topic_arn(&payload.topic_arn, "TopicArn")?;
            if payload.protocol.is_empty() {
                return Err(SnsError::BadRequest("Protocol is required".to_string()));
            }
            if payload.endpoint.is_empty() {
                return Err(SnsError::BadRequest("Endpoint is required".to_string()));
            }
            for key in payload.attributes.keys() {
                if !is_valid_subscription_attribute(key) {
                    return Err(SnsError::BadRequest(format!(
                        "Unsupported attribute name: {}",
                        key
                    )));
                }
                if let Some(value) = payload.attributes.get(key) {
                    validate_json_attribute(key, value)?;
                    if key == "RawMessageDelivery" {
                        validate_raw_message_delivery(value)?;
                    }
                }
            }
            Ok(())
        },
        handler: handle_subscribe_typed,
        span: "sns.subscription.create",
        span_attrs: |_request, payload, _store| {
            HashMap::from([
                ("topic_arn".to_string(), payload.topic_arn.clone()),
                ("protocol".to_string(), payload.protocol.clone()),
                ("endpoint".to_string(), payload.endpoint.clone()),
            ])
        },
        authorize_action: "sns:Subscribe",
        authorize_with: crate::auth::resolve_authorization,
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct SubscribeRequest {
    topic_arn: String,
    protocol: String,
    endpoint: String,
    #[serde(flatten, default)]
    attributes: SnsAttributes,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct SubscribeResult {
    subscription_arn: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct SubscribeResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    subscribe_result: SubscribeResult,
    response_metadata: ResponseMetadata,
}

async fn handle_subscribe_typed<S: SnsStore>(
    request: &ResolvedRequest,
    store: &S,
    request_body: SubscribeRequest,
) -> Result<SubscribeResponse, SnsError> {
    let topic = store
        .get_topic(&request_body.topic_arn)
        .await
        .map_err(|e| SnsError::InternalError(e.to_string()))?
        .ok_or(SnsError::TopicNotFound)?;

    if request_body.protocol != "sqs" {
        return Err(SnsError::BadRequest(format!(
            "Protocol '{}' is not supported in this slice",
            request_body.protocol
        )));
    }

    let subscription_arn = format!(
        "arn:aws:sns:{}:{}:{}:{}",
        topic.region,
        topic.account_id,
        topic.name,
        uuid::Uuid::new_v4()
    );

    let attrs = &request_body.attributes;
    let subscription = SnsSubscription {
        id: 0,
        topic_arn: request_body.topic_arn,
        protocol: request_body.protocol,
        endpoint: request_body.endpoint,
        owner_account_id: request.auth_context.principal.account_id.clone(),
        subscription_arn: subscription_arn.clone(),
        delivery_policy: attrs.get("DeliveryPolicy").map(|s| s.to_string()),
        filter_policy: attrs.get("FilterPolicy").map(|s| s.to_string()),
        filter_policy_scope: attrs.get("FilterPolicyScope").map(|s| s.to_string()),
        raw_message_delivery: attrs.get("RawMessageDelivery").map(|s| s.to_string()),
        redrive_policy: attrs.get("RedrivePolicy").map(|s| s.to_string()),
        subscription_role_arn: attrs.get("SubscriptionRoleArn").map(|s| s.to_string()),
        replay_policy: attrs.get("ReplayPolicy").map(|s| s.to_string()),
        created_at: Utc::now().naive_utc(),
    };

    store.create_subscription(subscription).await?;

    Ok(SubscribeResponse {
        xmlns: SNS_XMLNS,
        subscribe_result: SubscribeResult { subscription_arn },
        response_metadata: ResponseMetadata {
            request_id: request.request_id.clone(),
        },
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_core::{AuthContext, ResolvedRequest, TypedAwsAction};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::{
        principal::Principal,
        sns::{SnsSubscription, SnsTopic},
        test_support::SnsTestStore,
    };

    use super::{SubscribeAction, SubscribeRequest, handle_subscribe_typed};

    fn resolved_request(body: &str) -> ResolvedRequest {
        let mut headers = HashMap::new();
        headers.insert(
            "x-amz-target".to_string(),
            "AmazonSNS.Subscribe".to_string(),
        );

        ResolvedRequest {
            request_id: "test-request-id".to_string(),
            request: IncomingRequest {
                host: "localhost:4566".to_string(),
                method: "POST".to_string(),
                path: "/".to_string(),
                query: None,
                headers,
                body: body.as_bytes().to_vec(),
            },
            service: "sns".to_string(),
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
                        .with_ymd_and_hms(2026, 4, 4, 12, 0, 0)
                        .unwrap()
                        .naive_utc(),
                },
            },
            date: Utc.with_ymd_and_hms(2026, 4, 4, 12, 0, 0).unwrap(),
        }
    }

    fn topic() -> SnsTopic {
        SnsTopic {
            id: 1,
            name: "test-topic".to_string(),
            region: "us-east-1".to_string(),
            account_id: "123456789012".to_string(),
            display_name: None,
            policy: "{}".to_string(),
            delivery_policy: None,
            fifo_topic: None,
            signature_version: None,
            tracing_config: None,
            kms_master_key_id: None,
            data_protection_policy: None,
            archive_policy: None,
            beginning_archive_time: None,
            content_based_deduplication: None,
            created_at: Utc::now().naive_utc(),
        }
    }

    fn subscription() -> SnsSubscription {
        SnsSubscription {
            id: 1,
            topic_arn: "arn:aws:sns:us-east-1:123456789012:test-topic".to_string(),
            protocol: "sqs".to_string(),
            endpoint: "arn:aws:sqs:us-east-1:123456789012:test-queue".to_string(),
            owner_account_id: "123456789012".to_string(),
            subscription_arn: "arn:aws:sns:us-east-1:123456789012:test-topic:uuid-1".to_string(),
            delivery_policy: None,
            filter_policy: None,
            filter_policy_scope: None,
            raw_message_delivery: None,
            redrive_policy: None,
            subscription_role_arn: None,
            replay_policy: None,
            created_at: Utc::now().naive_utc(),
        }
    }

    #[test]
    fn reports_expected_action_name() {
        assert_eq!(
            <SubscribeAction as TypedAwsAction<SnsTestStore>>::name(&SubscribeAction),
            "Subscribe"
        );
    }

    #[tokio::test]
    async fn subscribe_to_existing_topic() {
        let store = SnsTestStore::with_topic(topic());
        let request = resolved_request(
            "TopicArn=arn:aws:sns:us-east-1:123456789012:test-topic&Protocol=sqs&Endpoint=arn:aws:sqs:us-east-1:123456789012:test-queue",
        );
        let body: SubscribeRequest = crate::actions::test_support::parse_request_body(&request);

        let response = handle_subscribe_typed(&request, &store, body)
            .await
            .expect("subscribe should succeed");

        assert!(
            response
                .subscribe_result
                .subscription_arn
                .starts_with("arn:aws:sns:us-east-1:123456789012:test-topic:")
        );

        let created = store.created_subscriptions();
        assert_eq!(created.len(), 1);
        assert_eq!(
            created[0].topic_arn,
            "arn:aws:sns:us-east-1:123456789012:test-topic"
        );
        assert_eq!(created[0].protocol, "sqs");
        assert_eq!(
            created[0].endpoint,
            "arn:aws:sqs:us-east-1:123456789012:test-queue"
        );
    }

    #[tokio::test]
    async fn validation_rejects_empty_fields() {
        let store = SnsTestStore::default();

        let request = resolved_request(
            "TopicArn=&Protocol=sqs&Endpoint=arn:aws:sqs:us-east-1:123456789012:test-queue",
        );
        let body: SubscribeRequest = crate::actions::test_support::parse_request_body(&request);
        let result = SubscribeAction.validate(&request, &body, &store).await;
        assert!(matches!(result, Err(crate::error::SnsError::BadRequest(_))));

        let request = resolved_request(
            "TopicArn=arn:aws:sns:us-east-1:123456789012:test-topic&Protocol=&Endpoint=arn:aws:sqs:us-east-1:123456789012:test-queue",
        );
        let body: SubscribeRequest = crate::actions::test_support::parse_request_body(&request);
        let result = SubscribeAction.validate(&request, &body, &store).await;
        assert!(matches!(result, Err(crate::error::SnsError::BadRequest(_))));

        let request = resolved_request(
            "TopicArn=arn:aws:sns:us-east-1:123456789012:test-topic&Protocol=sqs&Endpoint=",
        );
        let body: SubscribeRequest = crate::actions::test_support::parse_request_body(&request);
        let result = SubscribeAction.validate(&request, &body, &store).await;
        assert!(matches!(result, Err(crate::error::SnsError::BadRequest(_))));
    }

    #[tokio::test]
    async fn rejects_unsupported_protocols() {
        let store = SnsTestStore::with_topic(topic());
        let request = resolved_request(
            "TopicArn=arn:aws:sns:us-east-1:123456789012:test-topic&Protocol=http&Endpoint=http://example.com",
        );
        let body: SubscribeRequest = crate::actions::test_support::parse_request_body(&request);

        let result = handle_subscribe_typed(&request, &store, body).await;
        assert!(matches!(result, Err(crate::error::SnsError::BadRequest(_))));
    }
}
