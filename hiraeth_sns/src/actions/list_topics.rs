use std::collections::HashMap;

use hiraeth_core::ResolvedRequest;
use hiraeth_store::sns::{SnsStore, SnsTopic};
use serde::{Deserialize, Serialize};

use crate::{
    actions::action_support::{ResponseMetadata, SNS_XMLNS},
    error::SnsError,
};

const LIST_TOPICS_PAGE_SIZE: usize = 100;

pub(crate) struct ListTopicsAction;

hiraeth_core::impl_aws_action! {
    ListTopicsAction<S: SnsStore> {
        request: ListTopicsRequest,
        response: ListTopicsResponse,
        defaults: crate::SnsActionDefaults,
        name: "ListTopics",
        validate: |_request, payload, _store| {
            if let Some(next_token) = &payload.next_token
                && next_token.parse::<usize>().is_err()
            {
                return Err(SnsError::BadRequest(
                    "NextToken is invalid".to_string(),
                ));
            }
            Ok(())
        },
        handler: handle_list_topics,
        span: "sns.topic.list",
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
                action: "sns:ListTopics".to_string(),
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
pub(crate) struct ListTopicsRequest {
    #[serde(default)]
    next_token: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename = "ListTopicsResponse")]
#[serde(rename_all = "PascalCase")]
pub(crate) struct ListTopicsResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    list_topics_result: ListTopicsResult,
    response_metadata: ResponseMetadata,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
struct ListTopicsResult {
    topics: TopicList,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct TopicList {
    #[serde(rename = "member")]
    member: Vec<TopicSummary>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
struct TopicSummary {
    topic_arn: String,
}

async fn handle_list_topics<S: SnsStore>(
    request: &ResolvedRequest,
    store: &S,
    request_body: ListTopicsRequest,
) -> Result<ListTopicsResponse, SnsError> {
    let offset = request_body
        .next_token
        .as_deref()
        .map(str::parse::<usize>)
        .transpose()
        .map_err(|_| SnsError::BadRequest("NextToken is invalid".to_string()))?
        .unwrap_or(0);

    let account_id = &request.auth_context.principal.account_id;
    let all_topics = store
        .list_topics(&request.region, account_id, None, None)
        .await?
        .into_iter()
        .collect::<Vec<_>>();
    let next_offset = offset + LIST_TOPICS_PAGE_SIZE;
    let next_token = (next_offset < all_topics.len()).then(|| next_offset.to_string());
    let topics = all_topics
        .into_iter()
        .skip(offset)
        .take(LIST_TOPICS_PAGE_SIZE)
        .map(topic_summary)
        .collect();

    Ok(ListTopicsResponse {
        xmlns: SNS_XMLNS,
        list_topics_result: ListTopicsResult {
            topics: TopicList { member: topics },
            next_token,
        },
        response_metadata: ResponseMetadata {
            request_id: request.request_id.clone(),
        },
    })
}

fn topic_summary(topic: SnsTopic) -> TopicSummary {
    TopicSummary {
        topic_arn: format!(
            "arn:aws:sns:{}:{}:{}",
            topic.region, topic.account_id, topic.name
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_core::{AuthContext, ResolvedRequest, TypedAwsAction, xml_body};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::{principal::Principal, sns::SnsTopic, test_support::SnsTestStore};

    use super::{
        ListTopicsAction, ListTopicsRequest, ListTopicsResponse, ListTopicsResult, TopicList,
        TopicSummary, handle_list_topics,
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

    fn topic(name: &str, region: &str, account_id: &str) -> SnsTopic {
        SnsTopic {
            id: 1,
            name: name.to_string(),
            region: region.to_string(),
            account_id: account_id.to_string(),
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

    #[test]
    fn reports_expected_action_name() {
        assert_eq!(
            <ListTopicsAction as TypedAwsAction<SnsTestStore>>::name(&ListTopicsAction),
            "ListTopics"
        );
    }

    #[tokio::test]
    async fn returns_topics_for_request_scope() {
        let store = SnsTestStore::with_topics([
            topic("orders", "us-east-1", "123456789012"),
            topic("events", "us-east-1", "123456789012"),
            topic("other-region", "us-west-2", "123456789012"),
            topic("other-account", "us-east-1", "999999999999"),
        ]);
        let request = resolved_request("");
        let body = crate::actions::test_support::parse_request_body(&request);

        let response = handle_list_topics(&request, &store, body)
            .await
            .expect("list topics should succeed");

        assert_eq!(
            response.list_topics_result.topics.member,
            vec![
                TopicSummary {
                    topic_arn: "arn:aws:sns:us-east-1:123456789012:events".to_string(),
                },
                TopicSummary {
                    topic_arn: "arn:aws:sns:us-east-1:123456789012:orders".to_string(),
                },
            ]
        );
    }

    #[tokio::test]
    async fn returns_next_token_when_more_topics_remain() {
        let store = SnsTestStore::with_topics(
            (0..101).map(|index| topic(&format!("topic-{index:03}"), "us-east-1", "123456789012")),
        );
        let request = resolved_request("");
        let body = crate::actions::test_support::parse_request_body(&request);

        let response = handle_list_topics(&request, &store, body)
            .await
            .expect("list topics should succeed");

        assert_eq!(response.list_topics_result.topics.member.len(), 100);
        assert_eq!(
            response.list_topics_result.next_token,
            Some("100".to_string())
        );
    }

    #[tokio::test]
    async fn next_token_returns_later_page() {
        let store = SnsTestStore::with_topics(
            (0..101).map(|index| topic(&format!("topic-{index:03}"), "us-east-1", "123456789012")),
        );
        let request = resolved_request("NextToken=100");
        let body = crate::actions::test_support::parse_request_body(&request);

        let response = handle_list_topics(&request, &store, body)
            .await
            .expect("list topics should succeed");

        assert_eq!(
            response.list_topics_result.topics.member,
            vec![TopicSummary {
                topic_arn: "arn:aws:sns:us-east-1:123456789012:topic-100".to_string(),
            }]
        );
        assert_eq!(response.list_topics_result.next_token, None);
    }

    #[test]
    fn response_serializes_expected_xml_shape() {
        let response = ListTopicsResponse {
            xmlns: SNS_XMLNS,
            list_topics_result: ListTopicsResult {
                topics: TopicList {
                    member: vec![TopicSummary {
                        topic_arn: "arn:aws:sns:us-east-1:123456789012:orders".to_string(),
                    }],
                },
                next_token: None,
            },
            response_metadata: ResponseMetadata {
                request_id: "test-request-id".to_string(),
            },
        };

        let xml = String::from_utf8(xml_body(&response).unwrap()).unwrap();

        assert!(xml.contains("<ListTopicsResponse"));
        assert!(xml.contains("<ListTopicsResult>"));
        assert!(xml.contains("<Topics>"));
        assert!(xml.contains("<member>"));
        assert!(xml.contains("<TopicArn>arn:aws:sns:us-east-1:123456789012:orders</TopicArn>"));
        assert!(!xml.contains("<NextToken>"));
    }
}
