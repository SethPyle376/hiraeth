use std::collections::HashMap;

use hiraeth_core::ResolvedRequest;
use hiraeth_store::sns::SnsStore;
use serde::{Deserialize, Serialize};

use crate::{
    actions::action_support::{ResponseMetadata, SNS_XMLNS, validate_subscription_arn},
    error::SnsError,
};

pub(crate) struct UnsubscribeAction;

hiraeth_core::impl_aws_action! {
    UnsubscribeAction<S: SnsStore> {
        request: UnsubscribeRequest,
        response: UnsubscribeResponse,
        defaults: crate::SnsActionDefaults,
        name: "Unsubscribe",
        validate: |_request, payload, _store| {
            validate_subscription_arn(&payload.subscription_arn)
        },
        handler: handle_unsubscribe,
        span: "sns.subscription.delete",
        span_attrs: |_request, payload, _store| {
            HashMap::from([("subscription_arn".to_string(), payload.subscription_arn.clone())])
        },
        authorize_action: "sns:Unsubscribe",
        authorize_with: crate::auth::resolve_authorization,
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct UnsubscribeRequest {
    subscription_arn: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename = "UnsubscribeResponse")]
#[serde(rename_all = "PascalCase")]
pub(crate) struct UnsubscribeResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    response_metadata: ResponseMetadata,
}

async fn handle_unsubscribe<S: SnsStore>(
    request: &ResolvedRequest,
    store: &S,
    request_body: UnsubscribeRequest,
) -> Result<UnsubscribeResponse, SnsError> {
    store
        .get_subscription(&request_body.subscription_arn)
        .await?
        .ok_or(SnsError::SubscriptionNotFound)?;

    store
        .delete_subscription(&request_body.subscription_arn)
        .await
        .map_err(|error| match error {
            hiraeth_store::StoreError::NotFound(_) => SnsError::SubscriptionNotFound,
            other => SnsError::from(other),
        })?;

    Ok(UnsubscribeResponse {
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
        sns::{SnsStore, SnsSubscription},
        test_support::SnsTestStore,
    };

    use super::{UnsubscribeAction, UnsubscribeResponse, handle_unsubscribe};
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

    fn subscription(subscription_arn: &str) -> SnsSubscription {
        SnsSubscription {
            id: 1,
            topic_arn: "arn:aws:sns:us-east-1:123456789012:test-topic".to_string(),
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
            <UnsubscribeAction as TypedAwsAction<SnsTestStore>>::name(&UnsubscribeAction),
            "Unsubscribe"
        );
    }

    #[tokio::test]
    async fn deletes_existing_subscription() {
        let subscription_arn = "arn:aws:sns:us-east-1:123456789012:test-topic:sub-1";
        let store = SnsTestStore::with_subscription(subscription(subscription_arn));
        let request = resolved_request(&format!("SubscriptionArn={subscription_arn}"));
        let body = crate::actions::test_support::parse_request_body(&request);

        handle_unsubscribe(&request, &store, body)
            .await
            .expect("unsubscribe should succeed");

        assert!(
            store
                .get_subscription(subscription_arn)
                .await
                .expect("get subscription should succeed")
                .is_none()
        );
    }

    #[tokio::test]
    async fn returns_not_found_for_missing_subscription() {
        let store = SnsTestStore::default();
        let request =
            resolved_request("SubscriptionArn=arn:aws:sns:us-east-1:123456789012:test-topic:sub-1");
        let body = crate::actions::test_support::parse_request_body(&request);

        let result = handle_unsubscribe(&request, &store, body).await;

        assert!(matches!(result, Err(SnsError::SubscriptionNotFound)));
    }

    #[test]
    fn response_serializes_expected_xml_shape() {
        let response = UnsubscribeResponse {
            xmlns: SNS_XMLNS,
            response_metadata: ResponseMetadata {
                request_id: "test-request-id".to_string(),
            },
        };

        let xml = String::from_utf8(xml_body(&response).unwrap()).unwrap();

        assert!(xml.contains("<UnsubscribeResponse"));
        assert!(xml.contains("<ResponseMetadata>"));
        assert!(xml.contains("<RequestId>test-request-id</RequestId>"));
    }
}
