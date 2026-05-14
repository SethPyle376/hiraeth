use std::collections::HashMap;

use hiraeth_core::ResolvedRequest;
use hiraeth_store::sns::SnsStore;
use serde::{Deserialize, Serialize};

use crate::{
    actions::action_support::{ResponseMetadata, SNS_XMLNS, validate_topic_arn},
    error::SnsError,
};

pub(crate) struct DeleteTopicAction;

hiraeth_core::impl_aws_action! {
    DeleteTopicAction<S: SnsStore> {
        request: DeleteTopicRequest,
        response: DeleteTopicResponse,
        defaults: crate::SnsActionDefaults,
        name: "DeleteTopic",
        validate: |_request, payload, _store| {
            validate_topic_arn(&payload.topic_arn, "TopicArn")
        },
        handler: handle_delete_topic,
        span: "sns.topic.delete",
        span_attrs: |_request, payload, _store| {
            HashMap::from([("topic_arn".to_string(), payload.topic_arn.clone())])
        },
        authorize_action: "sns:DeleteTopic",
        authorize_with: crate::auth::resolve_authorization,
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct DeleteTopicRequest {
    topic_arn: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename = "DeleteTopicResponse")]
#[serde(rename_all = "PascalCase")]
pub(crate) struct DeleteTopicResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    response_metadata: ResponseMetadata,
}

async fn handle_delete_topic<S: SnsStore>(
    request: &ResolvedRequest,
    store: &S,
    request_body: DeleteTopicRequest,
) -> Result<DeleteTopicResponse, SnsError> {
    store.delete_topic(&request_body.topic_arn).await?;

    Ok(DeleteTopicResponse {
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

    use super::{DeleteTopicAction, DeleteTopicResponse, handle_delete_topic};
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
            <DeleteTopicAction as TypedAwsAction<SnsTestStore>>::name(&DeleteTopicAction),
            "DeleteTopic"
        );
    }

    #[tokio::test]
    async fn deletes_existing_topic() {
        let store = SnsTestStore::with_topic(topic());
        let topic_arn = "arn:aws:sns:us-east-1:123456789012:test-topic";
        let request = resolved_request(&format!("TopicArn={topic_arn}"));
        let body = crate::actions::test_support::parse_request_body(&request);

        let response = handle_delete_topic(&request, &store, body)
            .await
            .expect("delete topic should succeed");

        assert_eq!(response.response_metadata.request_id, "test-request-id");
        assert!(
            store
                .get_topic(topic_arn)
                .await
                .expect("get topic should succeed")
                .is_none()
        );
    }

    #[tokio::test]
    async fn returns_not_found_for_missing_topic() {
        let store = SnsTestStore::default();
        let request = resolved_request("TopicArn=arn:aws:sns:us-east-1:123456789012:test-topic");
        let body = crate::actions::test_support::parse_request_body(&request);

        let result = handle_delete_topic(&request, &store, body).await;

        assert!(matches!(result, Err(SnsError::TopicNotFound)));
    }

    #[test]
    fn response_serializes_expected_xml_shape() {
        let response = DeleteTopicResponse {
            xmlns: SNS_XMLNS,
            response_metadata: ResponseMetadata {
                request_id: "test-request-id".to_string(),
            },
        };

        let xml = String::from_utf8(xml_body(&response).unwrap()).unwrap();

        assert!(xml.contains("<DeleteTopicResponse"));
        assert!(xml.contains("<ResponseMetadata>"));
        assert!(xml.contains("<RequestId>test-request-id</RequestId>"));
    }
}
