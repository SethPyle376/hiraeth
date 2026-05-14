use std::collections::HashMap;

use hiraeth_core::ResolvedRequest;
use hiraeth_store::sns::SnsStore;
use serde::{Deserialize, Serialize};

use crate::{
    actions::action_support::{ResponseMetadata, SNS_XMLNS},
    error::SnsError,
};

pub(crate) struct GetSubscriptionAttributesAction;

hiraeth_core::impl_aws_action! {
    GetSubscriptionAttributesAction<S: SnsStore> {
        request: GetSubscriptionAttributesRequest,
        response: GetSubscriptionAttributesResponse,
        defaults: crate::SnsActionDefaults,
        name: "GetSubscriptionAttributes",
        validate: |_request, payload, _store| {
            if payload.subscription_arn.is_empty() {
                return Err(SnsError::BadRequest("SubscriptionArn is required".to_string()));
            }
            Ok(())
        },
        handler: handle_get_subscription_attributes,
        span: "sns.subscription.get_attributes",
        span_attrs: |_request, payload, _store| {
            HashMap::from([("subscription_arn".to_string(), payload.subscription_arn.clone())])
        },
        authorize_action: "sns:GetSubscriptionAttributes",
        authorize_with: crate::auth::resolve_authorization,
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct GetSubscriptionAttributesRequest {
    subscription_arn: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename = "GetSubscriptionAttributesResponse")]
#[serde(rename_all = "PascalCase")]
pub(crate) struct GetSubscriptionAttributesResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    get_subscription_attributes_result: GetSubscriptionAttributesResult,
    response_metadata: ResponseMetadata,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
struct GetSubscriptionAttributesResult {
    attributes: SubscriptionAttributes,
}

#[derive(Debug, Clone, Serialize)]
struct SubscriptionAttributes {
    entry: Vec<AttributeEntry>,
}

#[derive(Debug, Clone, Serialize)]
struct AttributeEntry {
    key: String,
    value: String,
}

async fn handle_get_subscription_attributes<S: SnsStore>(
    request: &ResolvedRequest,
    store: &S,
    request_body: GetSubscriptionAttributesRequest,
) -> Result<GetSubscriptionAttributesResponse, SnsError> {
    let subscription = store
        .get_subscription(&request_body.subscription_arn)
        .await?
        .ok_or(SnsError::SubscriptionNotFound)?;

    let mut entries = vec![
        attribute("SubscriptionArn", subscription.subscription_arn),
        attribute("TopicArn", subscription.topic_arn),
        attribute("Owner", subscription.owner_account_id),
        attribute("Protocol", subscription.protocol),
        attribute("Endpoint", subscription.endpoint),
        attribute("PendingConfirmation", "false"),
        attribute("ConfirmationWasAuthenticated", "true"),
        attribute(
            "RawMessageDelivery",
            subscription
                .raw_message_delivery
                .unwrap_or_else(|| "false".to_string()),
        ),
    ];

    push_optional_attribute(&mut entries, "DeliveryPolicy", subscription.delivery_policy);
    push_optional_attribute(&mut entries, "FilterPolicy", subscription.filter_policy);
    push_optional_attribute(
        &mut entries,
        "FilterPolicyScope",
        subscription.filter_policy_scope,
    );
    push_optional_attribute(&mut entries, "RedrivePolicy", subscription.redrive_policy);
    push_optional_attribute(
        &mut entries,
        "SubscriptionRoleArn",
        subscription.subscription_role_arn,
    );
    push_optional_attribute(&mut entries, "ReplayPolicy", subscription.replay_policy);

    Ok(GetSubscriptionAttributesResponse {
        xmlns: SNS_XMLNS,
        get_subscription_attributes_result: GetSubscriptionAttributesResult {
            attributes: SubscriptionAttributes { entry: entries },
        },
        response_metadata: ResponseMetadata {
            request_id: request.request_id.clone(),
        },
    })
}

fn attribute(key: impl Into<String>, value: impl Into<String>) -> AttributeEntry {
    AttributeEntry {
        key: key.into(),
        value: value.into(),
    }
}

fn push_optional_attribute(
    entries: &mut Vec<AttributeEntry>,
    key: &'static str,
    value: Option<String>,
) {
    if let Some(value) = value {
        entries.push(attribute(key, value));
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
        sns::{SnsStore, SnsSubscription},
        test_support::SnsTestStore,
    };

    use super::{
        AttributeEntry, GetSubscriptionAttributesAction, GetSubscriptionAttributesResponse,
        GetSubscriptionAttributesResult, SubscriptionAttributes,
        handle_get_subscription_attributes,
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

    fn subscription() -> SnsSubscription {
        SnsSubscription {
            id: 1,
            topic_arn: "arn:aws:sns:us-east-1:123456789012:test-topic".to_string(),
            protocol: "sqs".to_string(),
            endpoint: "arn:aws:sqs:us-east-1:123456789012:test-queue".to_string(),
            owner_account_id: "123456789012".to_string(),
            subscription_arn: "arn:aws:sns:us-east-1:123456789012:test-topic:uuid-1".to_string(),
            delivery_policy: Some(r#"{"healthyRetryPolicy":{"numRetries":3}}"#.to_string()),
            filter_policy: Some(r#"{"event":["created"]}"#.to_string()),
            filter_policy_scope: Some("MessageAttributes".to_string()),
            raw_message_delivery: Some("true".to_string()),
            redrive_policy: None,
            subscription_role_arn: None,
            replay_policy: None,
            created_at: Utc::now().naive_utc(),
        }
    }

    #[test]
    fn reports_expected_action_name() {
        assert_eq!(
            <GetSubscriptionAttributesAction as TypedAwsAction<SnsTestStore>>::name(
                &GetSubscriptionAttributesAction
            ),
            "GetSubscriptionAttributes"
        );
    }

    #[tokio::test]
    async fn returns_subscription_attributes() {
        let store = SnsTestStore::with_subscription(subscription());
        let request = resolved_request(
            "SubscriptionArn=arn:aws:sns:us-east-1:123456789012:test-topic:uuid-1",
        );
        let body = crate::actions::test_support::parse_request_body(&request);

        let response = handle_get_subscription_attributes(&request, &store, body)
            .await
            .expect("get subscription attributes should succeed");

        let attrs: HashMap<String, String> = response
            .get_subscription_attributes_result
            .attributes
            .entry
            .into_iter()
            .map(|entry| (entry.key, entry.value))
            .collect();

        assert_eq!(
            attrs.get("SubscriptionArn"),
            Some(&"arn:aws:sns:us-east-1:123456789012:test-topic:uuid-1".to_string())
        );
        assert_eq!(
            attrs.get("TopicArn"),
            Some(&"arn:aws:sns:us-east-1:123456789012:test-topic".to_string())
        );
        assert_eq!(attrs.get("Owner"), Some(&"123456789012".to_string()));
        assert_eq!(attrs.get("Protocol"), Some(&"sqs".to_string()));
        assert_eq!(
            attrs.get("Endpoint"),
            Some(&"arn:aws:sqs:us-east-1:123456789012:test-queue".to_string())
        );
        assert_eq!(attrs.get("RawMessageDelivery"), Some(&"true".to_string()));
        assert_eq!(
            attrs.get("FilterPolicy"),
            Some(&r#"{"event":["created"]}"#.to_string())
        );
        assert_eq!(
            attrs.get("FilterPolicyScope"),
            Some(&"MessageAttributes".to_string())
        );
    }

    #[tokio::test]
    async fn returns_not_found_for_missing_subscription() {
        let store = SnsTestStore::default();
        let request = resolved_request(
            "SubscriptionArn=arn:aws:sns:us-east-1:123456789012:test-topic:uuid-1",
        );
        let body = crate::actions::test_support::parse_request_body(&request);

        let result = handle_get_subscription_attributes(&request, &store, body).await;

        assert!(matches!(result, Err(SnsError::SubscriptionNotFound)));
    }

    #[test]
    fn response_serializes_expected_xml_shape() {
        let response = GetSubscriptionAttributesResponse {
            xmlns: SNS_XMLNS,
            get_subscription_attributes_result: GetSubscriptionAttributesResult {
                attributes: SubscriptionAttributes {
                    entry: vec![AttributeEntry {
                        key: "Protocol".to_string(),
                        value: "sqs".to_string(),
                    }],
                },
            },
            response_metadata: ResponseMetadata {
                request_id: "test-request-id".to_string(),
            },
        };

        let xml = String::from_utf8(xml_body(&response).unwrap()).unwrap();

        assert!(xml.contains("<GetSubscriptionAttributesResponse"));
        assert!(xml.contains("<GetSubscriptionAttributesResult>"));
        assert!(xml.contains("<Attributes>"));
        assert!(xml.contains("<entry>"));
        assert!(xml.contains("<key>Protocol</key>"));
        assert!(xml.contains("<value>sqs</value>"));
    }
}
