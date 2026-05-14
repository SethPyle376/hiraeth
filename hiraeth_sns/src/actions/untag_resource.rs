use std::collections::HashMap;

use hiraeth_core::ResolvedRequest;
use hiraeth_store::sns::SnsStore;
use serde::{Deserialize, Serialize};

use crate::{
    actions::action_support::{
        ResponseMetadata, SNS_XMLNS, SnsTagKeys, validate_tag_keys, validate_topic_arn,
    },
    error::SnsError,
};

pub(crate) struct UntagResourceAction;

hiraeth_core::impl_aws_action! {
    UntagResourceAction<S: SnsStore> {
        request: UntagResourceRequest,
        response: UntagResourceResponse,
        defaults: crate::SnsActionDefaults,
        name: "UntagResource",
        validate: |_request, payload, _store| {
            validate_topic_arn(&payload.resource_arn, "ResourceArn")?;
            validate_tag_keys(payload.tag_keys.as_slice(), false)
        },
        handler: handle_untag_resource,
        span: "sns.resource.untag",
        span_attrs: |_request, payload, _store| {
            HashMap::from([
                ("resource_arn".to_string(), payload.resource_arn.clone()),
                ("tag_key_count".to_string(), payload.tag_keys.len().to_string()),
                ("tag_keys".to_string(), payload.tag_keys.join(",")),
            ])
        },
        authorize_action: "sns:UntagResource",
        authorize_with: crate::auth::resolve_authorization,
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct UntagResourceRequest {
    resource_arn: String,
    #[serde(flatten, default)]
    tag_keys: SnsTagKeys,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename = "UntagResourceResponse")]
#[serde(rename_all = "PascalCase")]
pub(crate) struct UntagResourceResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    response_metadata: ResponseMetadata,
}

async fn handle_untag_resource<S: SnsStore>(
    request: &ResolvedRequest,
    store: &S,
    request_body: UntagResourceRequest,
) -> Result<UntagResourceResponse, SnsError> {
    let tag_keys = request_body.tag_keys.into_inner();
    validate_tag_keys(&tag_keys, false)?;

    store
        .get_topic(&request_body.resource_arn)
        .await?
        .ok_or(SnsError::TopicNotFound)?;

    store
        .untag_topic(&request_body.resource_arn, tag_keys)
        .await?;

    Ok(UntagResourceResponse {
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

    use super::{UntagResourceAction, UntagResourceResponse, handle_untag_resource};
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
            <UntagResourceAction as TypedAwsAction<SnsTestStore>>::name(&UntagResourceAction),
            "UntagResource"
        );
    }

    #[tokio::test]
    async fn removes_requested_tags() {
        let store = SnsTestStore::with_topic(topic());
        let arn = "arn:aws:sns:us-east-1:123456789012:test-topic";
        store
            .tag_topic(
                arn,
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
            "ResourceArn=arn:aws:sns:us-east-1:123456789012:test-topic&TagKeys.member.1=owner",
        );
        let body = crate::actions::test_support::parse_request_body(&request);

        handle_untag_resource(&request, &store, body)
            .await
            .expect("untag resource should succeed");

        assert_eq!(
            store.topic_tags(arn),
            [("environment".to_string(), "test".to_string())]
                .into_iter()
                .collect()
        );
    }

    #[tokio::test]
    async fn rejects_empty_tag_keys() {
        let store = SnsTestStore::with_topic(topic());
        let request = resolved_request("ResourceArn=arn:aws:sns:us-east-1:123456789012:test-topic");
        let body = crate::actions::test_support::parse_request_body(&request);

        let result = handle_untag_resource(&request, &store, body).await;

        assert!(matches!(result, Err(SnsError::BadRequest(_))));
    }

    #[test]
    fn response_serializes_expected_xml_shape() {
        let response = UntagResourceResponse {
            xmlns: SNS_XMLNS,
            response_metadata: ResponseMetadata {
                request_id: "test-request-id".to_string(),
            },
        };

        let xml = String::from_utf8(xml_body(&response).unwrap()).unwrap();

        assert!(xml.contains("<UntagResourceResponse"));
        assert!(xml.contains("<ResponseMetadata>"));
        assert!(xml.contains("<RequestId>test-request-id</RequestId>"));
    }
}
