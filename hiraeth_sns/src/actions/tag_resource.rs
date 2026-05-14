use std::collections::HashMap;

use hiraeth_core::ResolvedRequest;
use hiraeth_store::sns::SnsStore;
use serde::{Deserialize, Serialize};

use crate::{
    actions::action_support::{ResponseMetadata, SNS_XMLNS, SnsTags, validate_tags},
    error::SnsError,
};

pub(crate) struct TagResourceAction;

hiraeth_core::impl_aws_action! {
    TagResourceAction<S: SnsStore> {
        request: TagResourceRequest,
        response: TagResourceResponse,
        defaults: crate::SnsActionDefaults,
        name: "TagResource",
        validate: |_request, payload, _store| {
            validate_tags(&payload.tags.clone().into_inner(), false)
        },
        handler: handle_tag_resource,
        span: "sns.resource.tag",
        span_attrs: |_request, payload, _store| {
            HashMap::from([
                ("resource_arn".to_string(), payload.resource_arn.clone()),
                ("tag_count".to_string(), payload.tags.len().to_string()),
                (
                    "tag_keys".to_string(),
                    payload.tags.keys().cloned().collect::<Vec<_>>().join(","),
                ),
            ])
        },
        authorize_action: "sns:TagResource",
        authorize_with: crate::auth::resolve_authorization,
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct TagResourceRequest {
    resource_arn: String,
    #[serde(flatten, default)]
    tags: SnsTags,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename = "TagResourceResponse")]
#[serde(rename_all = "PascalCase")]
pub(crate) struct TagResourceResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    response_metadata: ResponseMetadata,
}

async fn handle_tag_resource<S: SnsStore>(
    request: &ResolvedRequest,
    store: &S,
    request_body: TagResourceRequest,
) -> Result<TagResourceResponse, SnsError> {
    let tags = request_body.tags.into_inner();
    validate_tags(&tags, false)?;

    store
        .get_topic(&request_body.resource_arn)
        .await?
        .ok_or(SnsError::TopicNotFound)?;

    let mut merged_tags = store.list_topic_tags(&request_body.resource_arn).await?;
    merged_tags.extend(tags.clone());
    validate_tags(&merged_tags, true)?;

    store.tag_topic(&request_body.resource_arn, tags).await?;

    Ok(TagResourceResponse {
        xmlns: SNS_XMLNS,
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

    use super::{TagResourceAction, TagResourceResponse, handle_tag_resource};
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
            <TagResourceAction as TypedAwsAction<SnsTestStore>>::name(&TagResourceAction),
            "TagResource"
        );
    }

    #[tokio::test]
    async fn merges_tags_for_existing_topic() {
        let store = SnsTestStore::with_topic(topic());
        let arn = "arn:aws:sns:us-east-1:123456789012:test-topic";
        store
            .tag_topic(
                arn,
                [("environment".to_string(), "test".to_string())]
                    .into_iter()
                    .collect(),
            )
            .await
            .expect("tags should seed");
        let request = resolved_request(
            "ResourceArn=arn:aws:sns:us-east-1:123456789012:test-topic&Tags.member.1.Key=environment&Tags.member.1.Value=prod&Tags.member.2.Key=owner&Tags.member.2.Value=hiraeth",
        );
        let body = crate::actions::test_support::parse_request_body(&request);

        handle_tag_resource(&request, &store, body)
            .await
            .expect("tag resource should succeed");

        assert_eq!(
            store.topic_tags(arn),
            [
                ("environment".to_string(), "prod".to_string()),
                ("owner".to_string(), "hiraeth".to_string()),
            ]
            .into_iter()
            .collect::<HashMap<_, _>>()
        );
    }

    #[tokio::test]
    async fn rejects_reserved_tag_key_prefix() {
        let store = SnsTestStore::with_topic(topic());
        let request = resolved_request(
            "ResourceArn=arn:aws:sns:us-east-1:123456789012:test-topic&Tags.member.1.Key=aws:reserved&Tags.member.1.Value=value",
        );
        let body = crate::actions::test_support::parse_request_body(&request);

        let result = handle_tag_resource(&request, &store, body).await;

        assert!(matches!(result, Err(SnsError::BadRequest(_))));
    }

    #[test]
    fn response_serializes_expected_xml_shape() {
        let response = TagResourceResponse {
            xmlns: SNS_XMLNS,
            response_metadata: ResponseMetadata {
                request_id: "test-request-id".to_string(),
            },
        };

        let xml = String::from_utf8(xml_body(&response).unwrap()).unwrap();

        assert!(xml.contains("<TagResourceResponse"));
        assert!(xml.contains("<ResponseMetadata>"));
        assert!(xml.contains("<RequestId>test-request-id</RequestId>"));
    }
}
