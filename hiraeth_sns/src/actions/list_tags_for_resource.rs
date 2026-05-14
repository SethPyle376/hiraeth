use std::collections::HashMap;

use hiraeth_core::ResolvedRequest;
use hiraeth_store::sns::SnsStore;
use serde::{Deserialize, Serialize};

use crate::{
    actions::action_support::{ResponseMetadata, SNS_XMLNS},
    error::SnsError,
};

pub(crate) struct ListTagsForResourceAction;

hiraeth_core::impl_aws_action! {
    ListTagsForResourceAction<S: SnsStore> {
        request: ListTagsForResourceRequest,
        response: ListTagsForResourceResponse,
        defaults: crate::SnsActionDefaults,
        name: "ListTagsForResource",
        validate: |_request, payload, _store| {
            if payload.resource_arn.is_empty() {
                return Err(SnsError::BadRequest("ResourceArn is required".to_string()));
            }
            Ok(())
        },
        handler: handle_list_tags_for_resource,
        span: "sns.resource.list_tags",
        span_attrs: |_request, payload, _store| {
            HashMap::from([("resource_arn".to_string(), payload.resource_arn.clone())])
        },
        authorize_action: "sns:ListTagsForResource",
        authorize_with: crate::auth::resolve_authorization,
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct ListTagsForResourceRequest {
    resource_arn: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename = "ListTagsForResourceResponse")]
#[serde(rename_all = "PascalCase")]
pub(crate) struct ListTagsForResourceResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    list_tags_for_resource_result: ListTagsForResourceResult,
    response_metadata: ResponseMetadata,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
struct ListTagsForResourceResult {
    tags: TagList,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct TagList {
    #[serde(rename = "member")]
    member: Vec<Tag>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
struct Tag {
    key: String,
    value: String,
}

async fn handle_list_tags_for_resource<S: SnsStore>(
    request: &ResolvedRequest,
    store: &S,
    request_body: ListTagsForResourceRequest,
) -> Result<ListTagsForResourceResponse, SnsError> {
    store
        .get_topic(&request_body.resource_arn)
        .await?
        .ok_or(SnsError::TopicNotFound)?;
    let mut tags = store
        .list_topic_tags(&request_body.resource_arn)
        .await?
        .into_iter()
        .map(|(key, value)| Tag { key, value })
        .collect::<Vec<_>>();
    tags.sort_by(|left, right| left.key.cmp(&right.key));

    Ok(ListTagsForResourceResponse {
        xmlns: SNS_XMLNS,
        list_tags_for_resource_result: ListTagsForResourceResult {
            tags: TagList { member: tags },
        },
        response_metadata: ResponseMetadata {
            request_id: request.request_id.clone(),
        },
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_core::{AuthContext, ResolvedRequest, TypedAwsAction, xml_body};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::{
        principal::Principal,
        sns::{SnsStore, SnsTopic},
        test_support::SnsTestStore,
    };

    use super::{
        ListTagsForResourceAction, ListTagsForResourceResponse, ListTagsForResourceResult, Tag,
        TagList, handle_list_tags_for_resource,
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

    #[test]
    fn reports_expected_action_name() {
        assert_eq!(
            <ListTagsForResourceAction as TypedAwsAction<SnsTestStore>>::name(
                &ListTagsForResourceAction
            ),
            "ListTagsForResource"
        );
    }

    #[tokio::test]
    async fn returns_tags_for_existing_topic() {
        let store = SnsTestStore::with_topic(topic());
        store
            .tag_topic(
                "arn:aws:sns:us-east-1:123456789012:test-topic",
                [
                    ("environment".to_string(), "test".to_string()),
                    ("owner".to_string(), "hiraeth".to_string()),
                ]
                .into_iter()
                .collect(),
            )
            .await
            .expect("tags should seed");
        let request = resolved_request("ResourceArn=arn:aws:sns:us-east-1:123456789012:test-topic");
        let body = crate::actions::test_support::parse_request_body(&request);

        let response = handle_list_tags_for_resource(&request, &store, body)
            .await
            .expect("list tags should succeed");

        assert_eq!(
            response.list_tags_for_resource_result.tags.member,
            vec![
                Tag {
                    key: "environment".to_string(),
                    value: "test".to_string(),
                },
                Tag {
                    key: "owner".to_string(),
                    value: "hiraeth".to_string(),
                },
            ]
        );
    }

    #[tokio::test]
    async fn returns_not_found_for_missing_topic() {
        let store = SnsTestStore::default();
        let request = resolved_request("ResourceArn=arn:aws:sns:us-east-1:123456789012:test-topic");
        let body = crate::actions::test_support::parse_request_body(&request);

        let result = handle_list_tags_for_resource(&request, &store, body).await;

        assert!(matches!(result, Err(SnsError::TopicNotFound)));
    }

    #[test]
    fn response_serializes_expected_xml_shape() {
        let response = ListTagsForResourceResponse {
            xmlns: SNS_XMLNS,
            list_tags_for_resource_result: ListTagsForResourceResult {
                tags: TagList {
                    member: vec![Tag {
                        key: "environment".to_string(),
                        value: "test".to_string(),
                    }],
                },
            },
            response_metadata: ResponseMetadata {
                request_id: "test-request-id".to_string(),
            },
        };

        let xml = String::from_utf8(xml_body(&response).unwrap()).unwrap();

        assert!(xml.contains("<ListTagsForResourceResponse"));
        assert!(xml.contains("<ListTagsForResourceResult>"));
        assert!(xml.contains("<Tags>"));
        assert!(xml.contains("<member>"));
        assert!(xml.contains("<Key>environment</Key>"));
        assert!(xml.contains("<Value>test</Value>"));
    }
}
