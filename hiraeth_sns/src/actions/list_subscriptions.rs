use std::collections::HashMap;

use hiraeth_core::ResolvedRequest;
use hiraeth_store::sns::{SnsStore, SnsSubscription};
use serde::{Deserialize, Serialize};

use crate::{
    actions::action_support::{ResponseMetadata, SNS_XMLNS},
    error::SnsError,
};

const LIST_SUBSCRIPTIONS_PAGE_SIZE: usize = 100;

pub(crate) struct ListSubscriptionsAction;

hiraeth_core::impl_aws_action! {
    ListSubscriptionsAction<S: SnsStore> {
        request: ListSubscriptionsRequest,
        response: ListSubscriptionsResponse,
        defaults: crate::SnsActionDefaults,
        name: "ListSubscriptions",
        validate: |_request, payload, _store| {
            if let Some(next_token) = &payload.next_token
                && next_token.parse::<usize>().is_err()
            {
                return Err(SnsError::BadRequest("NextToken is invalid".to_string()));
            }
            Ok(())
        },
        handler: handle_list_subscriptions,
        span: "sns.subscription.list",
        span_attrs: |request, _payload, _store| {
            HashMap::from([
                ("region".to_string(), request.region.clone()),
                (
                    "account_id".to_string(),
                    request.auth_context.principal.account_id.clone(),
                ),
            ])
        },
        authorize: |request, _payload, _store| {
            Ok(hiraeth_core::auth::AuthorizationCheck {
                action: "sns:ListSubscriptions".to_string(),
                resource: format!(
                    "arn:aws:sns:{}:{}:*",
                    request.region, request.auth_context.principal.account_id
                ),
                resource_policy: None,
            })
        },
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct ListSubscriptionsRequest {
    #[serde(default)]
    next_token: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename = "ListSubscriptionsResponse")]
#[serde(rename_all = "PascalCase")]
pub(crate) struct ListSubscriptionsResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    list_subscriptions_result: ListSubscriptionsResult,
    response_metadata: ResponseMetadata,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
struct ListSubscriptionsResult {
    subscriptions: SubscriptionList,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct SubscriptionList {
    #[serde(rename = "member")]
    member: Vec<SubscriptionSummary>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
struct SubscriptionSummary {
    subscription_arn: String,
    owner: String,
    protocol: String,
    endpoint: String,
    topic_arn: String,
}

async fn handle_list_subscriptions<S: SnsStore>(
    request: &ResolvedRequest,
    store: &S,
    request_body: ListSubscriptionsRequest,
) -> Result<ListSubscriptionsResponse, SnsError> {
    let offset = request_body
        .next_token
        .as_deref()
        .map(str::parse::<usize>)
        .transpose()
        .map_err(|_| SnsError::BadRequest("NextToken is invalid".to_string()))?
        .unwrap_or(0);

    let account_id = &request.auth_context.principal.account_id;
    let all_subscriptions = store
        .list_subscriptions(&request.region, account_id, None)
        .await?;
    let next_offset = offset + LIST_SUBSCRIPTIONS_PAGE_SIZE;
    let next_token = (next_offset < all_subscriptions.len()).then(|| next_offset.to_string());
    let subscriptions = all_subscriptions
        .into_iter()
        .skip(offset)
        .take(LIST_SUBSCRIPTIONS_PAGE_SIZE)
        .map(subscription_summary)
        .collect();

    Ok(ListSubscriptionsResponse {
        xmlns: SNS_XMLNS,
        list_subscriptions_result: ListSubscriptionsResult {
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
    use hiraeth_store::{principal::Principal, sns::SnsSubscription, test_support::SnsTestStore};

    use super::{
        ListSubscriptionsAction, ListSubscriptionsResponse, ListSubscriptionsResult,
        SubscriptionList, SubscriptionSummary, handle_list_subscriptions,
    };
    use crate::actions::action_support::{ResponseMetadata, SNS_XMLNS};

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

    fn subscription(subscription_arn: &str, topic_arn: &str) -> SnsSubscription {
        SnsSubscription {
            id: 1,
            topic_arn: topic_arn.to_string(),
            protocol: "sqs".to_string(),
            endpoint: "arn:aws:sqs:us-east-1:123456789012:test-queue".to_string(),
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
            <ListSubscriptionsAction as TypedAwsAction<SnsTestStore>>::name(
                &ListSubscriptionsAction
            ),
            "ListSubscriptions"
        );
    }

    #[tokio::test]
    async fn returns_subscriptions_for_request_scope() {
        let store = SnsTestStore::with_subscriptions([
            subscription(
                "arn:aws:sns:us-east-1:123456789012:topic-a:sub-1",
                "arn:aws:sns:us-east-1:123456789012:topic-a",
            ),
            subscription(
                "arn:aws:sns:us-east-1:123456789012:topic-b:sub-1",
                "arn:aws:sns:us-east-1:123456789012:topic-b",
            ),
            subscription(
                "arn:aws:sns:us-west-2:123456789012:topic-c:sub-1",
                "arn:aws:sns:us-west-2:123456789012:topic-c",
            ),
        ]);
        let request = resolved_request("");
        let body = crate::actions::test_support::parse_request_body(&request);

        let response = handle_list_subscriptions(&request, &store, body)
            .await
            .expect("list subscriptions should succeed");

        assert_eq!(
            response
                .list_subscriptions_result
                .subscriptions
                .member
                .len(),
            2
        );
    }

    #[test]
    fn response_serializes_expected_xml_shape() {
        let response = ListSubscriptionsResponse {
            xmlns: SNS_XMLNS,
            list_subscriptions_result: ListSubscriptionsResult {
                subscriptions: SubscriptionList {
                    member: vec![SubscriptionSummary {
                        subscription_arn: "arn:aws:sns:us-east-1:123456789012:topic-a:sub-1"
                            .to_string(),
                        owner: "123456789012".to_string(),
                        protocol: "sqs".to_string(),
                        endpoint: "arn:aws:sqs:us-east-1:123456789012:test-queue".to_string(),
                        topic_arn: "arn:aws:sns:us-east-1:123456789012:topic-a".to_string(),
                    }],
                },
                next_token: None,
            },
            response_metadata: ResponseMetadata {
                request_id: "test-request-id".to_string(),
            },
        };

        let xml = String::from_utf8(xml_body(&response).unwrap()).unwrap();

        assert!(xml.contains("<ListSubscriptionsResponse"));
        assert!(xml.contains("<ListSubscriptionsResult>"));
        assert!(xml.contains("<Subscriptions>"));
        assert!(xml.contains("<member>"));
        assert!(xml.contains(
            "<SubscriptionArn>arn:aws:sns:us-east-1:123456789012:topic-a:sub-1</SubscriptionArn>"
        ));
    }
}
