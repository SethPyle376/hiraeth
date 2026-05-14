use std::collections::HashMap;

use hiraeth_core::ResolvedRequest;
use hiraeth_store::{
    sns::{SnsStore, SnsTopicAttributeUpdate},
    sqs::SqsStore,
};
use serde::{Deserialize, Serialize};

use crate::{
    SnsServiceStore,
    actions::action_support::{
        ResponseMetadata, SNS_XMLNS, is_valid_topic_attribute, parse_sns_topic_arn,
    },
    error::SnsError,
};

pub(crate) struct SetTopicAttributesAction;

hiraeth_core::impl_aws_action! {
    SetTopicAttributesAction<SnsServiceStore<SS, QS>> where SS: SnsStore, QS: SqsStore {
        request: SetTopicAttributesRequest,
        response: SetTopicAttributesResponse,
        defaults: crate::SnsActionDefaults,
        name: "SetTopicAttributes",
        validate: |_request, payload, _store| {
            if is_valid_topic_attribute(&payload.attribute_name) {
                Ok(())
            } else {
                Err(SnsError::BadRequest(format!(
                    "Unsupported attribute name: {}",
                    payload.attribute_name
                )))
            }
        },
        handler: handle_set_topic_attributes,
        span: "sns.topic_attributes.set",
        span_attrs: |_request, payload, _store| {
            HashMap::from([
                ("topic_arn".to_string(), payload.topic_arn.clone()),
                ("attribute_name".to_string(), payload.attribute_name.clone()),
                ("attribute_value".to_string(), payload.attribute_value.clone()),
            ])
        },
        authorize: |request, _payload, store| {
            crate::auth::resolve_authorization(
                "sns:SetTopicAttributes",
                request,
                &store.sns_store,
            )
            .await
        },
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct SetTopicAttributesRequest {
    pub topic_arn: String,
    pub attribute_name: String,
    pub attribute_value: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct SetTopicAttributesResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    response_metadata: ResponseMetadata,
}

async fn handle_set_topic_attributes<SS, QS>(
    request: &ResolvedRequest,
    store: &SnsServiceStore<SS, QS>,
    request_body: SetTopicAttributesRequest,
) -> Result<SetTopicAttributesResponse, SnsError>
where
    SS: SnsStore + Send + Sync,
    QS: SqsStore + Send + Sync,
{
    let update = SnsTopicAttributeUpdate::from_attribute_name_and_value(
        &request_body.attribute_name,
        &request_body.attribute_value,
    );

    let topic_id = parse_sns_topic_arn(&request_body.topic_arn)
        .ok_or_else(|| SnsError::BadRequest("Invalid TopicArn format".to_string()))?;

    store
        .sns_store
        .set_topic_attributes(
            &topic_id.account_id,
            &topic_id.region,
            &topic_id.name,
            update,
        )
        .await?;

    Ok(SetTopicAttributesResponse {
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
    use hiraeth_core::tracing::{NoopTraceRecorder, TraceContext};
    use hiraeth_core::{AuthContext, ResolvedRequest, TypedAwsAction};
    use hiraeth_http::IncomingRequest;
    use hiraeth_iam::AuthorizationMode;
    use hiraeth_store::{
        principal::Principal,
        sns::{SnsStore, SnsTopic, SnsTopicAttributeUpdate},
        test_support::{SnsTestStore, SqsTestStore},
    };

    use super::{SetTopicAttributesAction, SetTopicAttributesRequest, handle_set_topic_attributes};
    use crate::store::SnsServiceStore;

    fn resolved_request(body: &str) -> ResolvedRequest {
        let mut headers = HashMap::new();
        headers.insert(
            "x-amz-target".to_string(),
            "AmazonSNS.SetTopicAttributes".to_string(),
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

    fn service_store(
        sns: SnsTestStore,
        sqs: SqsTestStore,
    ) -> SnsServiceStore<SnsTestStore, SqsTestStore> {
        SnsServiceStore::new(sns, sqs, AuthorizationMode::Off)
    }

    #[test]
    fn reports_expected_action_name() {
        assert_eq!(
            <SetTopicAttributesAction as TypedAwsAction<
                SnsServiceStore<SnsTestStore, SqsTestStore>,
            >>::name(&SetTopicAttributesAction),
            "SetTopicAttributes"
        );
    }

    #[tokio::test]
    async fn set_topic_attribute_updates_display_name() {
        let sns = SnsTestStore::with_topic(topic());
        let sqs = SqsTestStore::default();
        let store = service_store(sns, sqs);
        let request = resolved_request(
            "TopicArn=arn:aws:sns:us-east-1:123456789012:test-topic&AttributeName=DisplayName&AttributeValue=MyDisplay",
        );
        let body: SetTopicAttributesRequest =
            crate::actions::test_support::parse_request_body(&request);
        let trace_context = TraceContext::new("test-request-id");

        let response = handle_set_topic_attributes(&request, &store, body)
            .await
            .expect("set topic attributes should succeed");

        assert_eq!(response.response_metadata.request_id, "test-request-id");

        let updated = store
            .sns_store
            .get_topic("arn:aws:sns:us-east-1:123456789012:test-topic")
            .await
            .expect("get topic should succeed")
            .expect("topic should exist");
        assert_eq!(updated.display_name, Some("MyDisplay".to_string()));
    }

    #[tokio::test]
    async fn set_topic_attribute_updates_policy() {
        let sns = SnsTestStore::with_topic(topic());
        let sqs = SqsTestStore::default();
        let store = service_store(sns, sqs);
        let request = resolved_request(
            "TopicArn=arn:aws:sns:us-east-1:123456789012:test-topic&AttributeName=Policy&AttributeValue=%7B%22Statement%22%3A%5B%5D%7D",
        );
        let body: SetTopicAttributesRequest =
            crate::actions::test_support::parse_request_body(&request);
        let trace_context = TraceContext::new("test-request-id");

        let response = handle_set_topic_attributes(&request, &store, body)
            .await
            .expect("set topic attributes should succeed");

        assert_eq!(response.response_metadata.request_id, "test-request-id");

        let updated = store
            .sns_store
            .get_topic("arn:aws:sns:us-east-1:123456789012:test-topic")
            .await
            .expect("get topic should succeed")
            .expect("topic should exist");
        assert_eq!(updated.policy, r#"{"Statement":[]}"#);
    }

    #[tokio::test]
    async fn topic_not_found_error() {
        let sns = SnsTestStore::default();
        let sqs = SqsTestStore::default();
        let store = service_store(sns, sqs);
        let request = resolved_request(
            "TopicArn=arn:aws:sns:us-east-1:123456789012:test-topic&AttributeName=DisplayName&AttributeValue=MyDisplay",
        );
        let body: SetTopicAttributesRequest =
            crate::actions::test_support::parse_request_body(&request);
        let trace_context = TraceContext::new("test-request-id");

        let result = handle_set_topic_attributes(&request, &store, body).await;
        assert!(matches!(result, Err(crate::error::SnsError::TopicNotFound)));
    }

    #[tokio::test]
    async fn validation_accepts_feedback_attributes() {
        let sns = SnsTestStore::with_topic(topic());
        let sqs = SqsTestStore::default();
        let store = service_store(sns, sqs);

        let feedback_attrs = [
            "HTTPSuccessFeedbackRoleArn",
            "HTTPSuccessFeedbackSampleRate",
            "HTTPFailureFeedbackRoleArn",
            "FirehoseSuccessFeedbackRoleArn",
            "FirehoseSuccessFeedbackSampleRate",
            "FirehoseFailureFeedbackRoleArn",
            "LambdaSuccessFeedbackRoleArn",
            "LambdaSuccessFeedbackSampleRate",
            "LambdaFailureFeedbackRoleArn",
            "ApplicationSuccessFeedbackRoleArn",
            "ApplicationSuccessFeedbackSampleRate",
            "ApplicationFailureFeedbackRoleArn",
            "SQSSuccessFeedbackRoleArn",
            "SQSSuccessFeedbackSampleRate",
            "SQSFailureFeedbackRoleArn",
        ];

        for attr in feedback_attrs {
            let request = resolved_request(&format!(
                "TopicArn=arn:aws:sns:us-east-1:123456789012:test-topic&AttributeName={}&AttributeValue=value",
                attr
            ));
            let body: SetTopicAttributesRequest =
                crate::actions::test_support::parse_request_body(&request);

            let result = SetTopicAttributesAction
                .validate(&request, &body, &store)
                .await;
            assert!(result.is_ok(), "expected {} to pass validation", attr);
        }
    }

    #[tokio::test]
    async fn validation_rejects_unsupported_attribute() {
        let sns = SnsTestStore::with_topic(topic());
        let sqs = SqsTestStore::default();
        let store = service_store(sns, sqs);
        let request = resolved_request(
            "TopicArn=arn:aws:sns:us-east-1:123456789012:test-topic&AttributeName=UnknownAttr&AttributeValue=value",
        );
        let body: SetTopicAttributesRequest =
            crate::actions::test_support::parse_request_body(&request);

        let result = SetTopicAttributesAction
            .validate(&request, &body, &store)
            .await;
        assert!(matches!(result, Err(crate::error::SnsError::BadRequest(_))));
    }

    #[tokio::test]
    async fn invalid_topic_arn_returns_bad_request() {
        let sns = SnsTestStore::with_topic(topic());
        let sqs = SqsTestStore::default();
        let store = service_store(sns, sqs);
        let request = resolved_request(
            "TopicArn=not-an-arn&AttributeName=DisplayName&AttributeValue=MyDisplay",
        );
        let body: SetTopicAttributesRequest =
            crate::actions::test_support::parse_request_body(&request);
        let trace_context = TraceContext::new("test-request-id");

        let result = handle_set_topic_attributes(&request, &store, body).await;
        assert!(matches!(result, Err(crate::error::SnsError::BadRequest(_))));
    }
}
