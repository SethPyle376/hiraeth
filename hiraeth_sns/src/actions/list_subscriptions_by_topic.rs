use std::collections::HashMap;

use hiraeth_core::ResolvedRequest;
use hiraeth_store::sns::{SnsStore, SnsSubscription};
use serde::{Deserialize, Serialize};

use crate::{
    actions::action_support::{ResponseMetadata, SNS_XMLNS, validate_topic_arn},
    error::SnsError,
};

const LIST_SUBSCRIPTIONS_BY_TOPIC_PAGE_SIZE: usize = 100;

pub(crate) struct ListSubscriptionsByTopicAction;

hiraeth_core::impl_aws_action! {
    ListSubscriptionsByTopicAction<S: SnsStore> {
        request: ListSubscriptionsByTopicRequest,
        response: ListSubscriptionsByTopicResponse,
        defaults: crate::SnsActionDefaults,
        name: "ListSubscriptionsByTopic",
        validate: |_request, payload, _store| {
            validate_topic_arn(&payload.topic_arn, "TopicArn")?;
            if let Some(next_token) = &payload.next_token
                && next_token.parse::<usize>().is_err()
            {
                return Err(SnsError::BadRequest("NextToken is invalid".to_string()));
            }
            Ok(())
        },
        handler: handle_list_subscriptions_by_topic,
        span: "sns.topic.list_subscriptions",
        span_attrs: |_request, payload, _store| {
            HashMap::from([("topic_arn".to_string(), payload.topic_arn.clone())])
        },
        authorize_action: "sns:ListSubscriptionsByTopic",
        authorize_with: crate::auth::resolve_authorization,
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct ListSubscriptionsByTopicRequest {
    topic_arn: String,
    #[serde(default)]
    next_token: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename = "ListSubscriptionsByTopicResponse")]
#[serde(rename_all = "PascalCase")]
pub(crate) struct ListSubscriptionsByTopicResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    list_subscriptions_by_topic_result: ListSubscriptionsByTopicResult,
    response_metadata: ResponseMetadata,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
struct ListSubscriptionsByTopicResult {
    subscriptions: SubscriptionList,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_token: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct SubscriptionList {
    #[serde(rename = "member")]
    member: Vec<SubscriptionSummary>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
struct SubscriptionSummary {
    subscription_arn: String,
    owner: String,
    protocol: String,
    endpoint: String,
    topic_arn: String,
}

async fn handle_list_subscriptions_by_topic<S: SnsStore>(
    request: &ResolvedRequest,
    store: &S,
    request_body: ListSubscriptionsByTopicRequest,
) -> Result<ListSubscriptionsByTopicResponse, SnsError> {
    let offset = request_body
        .next_token
        .as_deref()
        .map(str::parse::<usize>)
        .transpose()
        .map_err(|_| SnsError::BadRequest("NextToken is invalid".to_string()))?
        .unwrap_or(0);

    store
        .get_topic(&request_body.topic_arn)
        .await?
        .ok_or(SnsError::TopicNotFound)?;

    let all_subscriptions = store
        .list_subscriptions_by_topic(&request_body.topic_arn)
        .await?;
    let next_offset = offset + LIST_SUBSCRIPTIONS_BY_TOPIC_PAGE_SIZE;
    let next_token = (next_offset < all_subscriptions.len()).then(|| next_offset.to_string());
    let subscriptions = all_subscriptions
        .into_iter()
        .skip(offset)
        .take(LIST_SUBSCRIPTIONS_BY_TOPIC_PAGE_SIZE)
        .map(subscription_summary)
        .collect();

    Ok(ListSubscriptionsByTopicResponse {
        xmlns: SNS_XMLNS,
        list_subscriptions_by_topic_result: ListSubscriptionsByTopicResult {
            subscriptions: SubscriptionList {
                member: subscriptions,
            },
            next_token,
        },
        response_metadata: ResponseMetadata {
            request_id: request.request_id.clone(),
        },
    })
}

fn subscription_summary(subscription: SnsSubscription) -> SubscriptionSummary {
    SubscriptionSummary {
        subscription_arn: subscription.subscription_arn,
        owner: subscription.owner_account_id,
        protocol: subscription.protocol,
        endpoint: subscription.endpoint,
        topic_arn: subscription.topic_arn,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_core::{AuthContext, ResolvedRequest, TypedAwsAction, xml_body};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::{
        principal::Principal,
        sns::{SnsStore, SnsSubscription, SnsTopic},
        test_support::SnsTestStore,
    };

    use super::{
        ListSubscriptionsByTopicAction, ListSubscriptionsByTopicResponse,
        ListSubscriptionsByTopicResult, SubscriptionList, SubscriptionSummary,
        handle_list_subscriptions_by_topic,
    };
    use crate::{
        actions::action_support::{ResponseMetadata, SNS_XMLNS},
        error::SnsError,
    };

    fn resolved_request(body: &str) -> ResolvedRequest {
        ResolvedRequest {
            request_id: "test-request-id".to_string(),
            request: IncomingRequest {
                host: "localhost:4566".to_string(),
                method: "POST".to_string(),
                path: "/".to_string(),
                query: None,
                headers: HashMap::new(),
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

    fn subscription(subscription_arn: &str, endpoint: &str) -> SnsSubscription {
        SnsSubscription {
            id: 1,
            topic_arn: "arn:aws:sns:us-east-1:123456789012:test-topic".to_string(),
            protocol: "sqs".to_string(),
            endpoint: endpoint.to_string(),
            owner_account_id: "123456789012".to_string(),
            subscription_arn: subscription_arn.to_string(),
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
            <ListSubscriptionsByTopicAction as TypedAwsAction<SnsTestStore>>::name(
                &ListSubscriptionsByTopicAction
            ),
            "ListSubscriptionsByTopic"
        );
    }

    #[tokio::test]
    async fn returns_subscriptions_for_topic() {
        let store = SnsTestStore::with_topics([topic()]);
        store
            .create_subscription(subscription(
                "arn:aws:sns:us-east-1:123456789012:test-topic:sub-1",
                "arn:aws:sqs:us-east-1:123456789012:test-queue-1",
            ))
            .await
            .expect("subscription should seed");
        store
            .create_subscription(subscription(
                "arn:aws:sns:us-east-1:123456789012:test-topic:sub-2",
                "arn:aws:sqs:us-east-1:123456789012:test-queue-2",
            ))
            .await
            .expect("subscription should seed");
        let request = resolved_request("TopicArn=arn:aws:sns:us-east-1:123456789012:test-topic");
        let body = crate::actions::test_support::parse_request_body(&request);

        let response = handle_list_subscriptions_by_topic(&request, &store, body)
            .await
            .expect("list subscriptions should succeed");

        let subscriptions = response
            .list_subscriptions_by_topic_result
            .subscriptions
            .member;

        assert_eq!(subscriptions.len(), 2);
        assert_eq!(
            subscriptions[0].topic_arn,
            "arn:aws:sns:us-east-1:123456789012:test-topic"
        );
        assert_eq!(subscriptions[0].protocol, "sqs");
        assert_eq!(
            subscriptions[0].endpoint,
            "arn:aws:sqs:us-east-1:123456789012:test-queue-1"
        );
    }

    #[tokio::test]
    async fn returns_empty_list_for_topic_without_subscriptions() {
        let store = SnsTestStore::with_topic(topic());
        let request = resolved_request("TopicArn=arn:aws:sns:us-east-1:123456789012:test-topic");
        let body = crate::actions::test_support::parse_request_body(&request);

        let response = handle_list_subscriptions_by_topic(&request, &store, body)
            .await
            .expect("list subscriptions should succeed");

        assert!(
            response
                .list_subscriptions_by_topic_result
                .subscriptions
                .member
                .is_empty()
        );
    }

    #[tokio::test]
    async fn returns_next_token_when_more_subscriptions_remain() {
        let store = SnsTestStore::with_topics([topic()]);
        for index in 0..101 {
            store
                .create_subscription(subscription(
                    &format!("arn:aws:sns:us-east-1:123456789012:test-topic:sub-{index:03}"),
                    "arn:aws:sqs:us-east-1:123456789012:test-queue",
                ))
                .await
                .expect("subscription should seed");
        }
        let request = resolved_request("TopicArn=arn:aws:sns:us-east-1:123456789012:test-topic");
        let body = crate::actions::test_support::parse_request_body(&request);

        let response = handle_list_subscriptions_by_topic(&request, &store, body)
            .await
            .expect("list subscriptions should succeed");

        assert_eq!(
            response
                .list_subscriptions_by_topic_result
                .subscriptions
                .member
                .len(),
            100
        );
        assert_eq!(
            response.list_subscriptions_by_topic_result.next_token,
            Some("100".to_string())
        );
    }

    #[tokio::test]
    async fn returns_not_found_for_missing_topic() {
        let store = SnsTestStore::default();
        let request = resolved_request("TopicArn=arn:aws:sns:us-east-1:123456789012:test-topic");
        let body = crate::actions::test_support::parse_request_body(&request);

        let result = handle_list_subscriptions_by_topic(&request, &store, body).await;

        assert!(matches!(result, Err(SnsError::TopicNotFound)));
    }

    #[test]
    fn response_serializes_expected_xml_shape() {
        let response = ListSubscriptionsByTopicResponse {
            xmlns: SNS_XMLNS,
            list_subscriptions_by_topic_result: ListSubscriptionsByTopicResult {
                subscriptions: SubscriptionList {
                    member: vec![SubscriptionSummary {
                        subscription_arn: "arn:aws:sns:us-east-1:123456789012:test-topic:sub-1"
                            .to_string(),
                        owner: "123456789012".to_string(),
                        protocol: "sqs".to_string(),
                        endpoint: "arn:aws:sqs:us-east-1:123456789012:test-queue".to_string(),
                        topic_arn: "arn:aws:sns:us-east-1:123456789012:test-topic".to_string(),
                    }],
                },
                next_token: None,
            },
            response_metadata: ResponseMetadata {
                request_id: "test-request-id".to_string(),
            },
        };

        let xml = String::from_utf8(xml_body(&response).unwrap()).unwrap();

        assert!(xml.contains("<ListSubscriptionsByTopicResponse"));
        assert!(xml.contains("<ListSubscriptionsByTopicResult>"));
        assert!(xml.contains("<Subscriptions>"));
        assert!(xml.contains("<member>"));
        assert!(xml.contains(
            "<SubscriptionArn>arn:aws:sns:us-east-1:123456789012:test-topic:sub-1</SubscriptionArn>"
        ));
        assert!(xml.contains("<Protocol>sqs</Protocol>"));
        assert!(xml.contains("<Endpoint>arn:aws:sqs:us-east-1:123456789012:test-queue</Endpoint>"));
        assert!(!xml.contains("<NextToken>"));
    }
}
