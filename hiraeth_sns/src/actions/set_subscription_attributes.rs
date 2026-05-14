use std::collections::HashMap;

use hiraeth_core::ResolvedRequest;
use hiraeth_store::sns::{SnsStore, SnsSubscriptionAttributeUpdate};
use serde::{Deserialize, Serialize};

use crate::{
    actions::action_support::{
        ResponseMetadata, SNS_XMLNS, is_valid_subscription_attribute, validate_json_attribute,
        validate_raw_message_delivery, validate_subscription_arn,
    },
    error::SnsError,
};

pub(crate) struct SetSubscriptionAttributesAction;

hiraeth_core::impl_aws_action! {
    SetSubscriptionAttributesAction<S: SnsStore> {
        request: SetSubscriptionAttributesRequest,
        response: SetSubscriptionAttributesResponse,
        defaults: crate::SnsActionDefaults,
        name: "SetSubscriptionAttributes",
        validate: |_request, payload, _store| {
            validate_subscription_arn(&payload.subscription_arn)?;
            if payload.attribute_name.is_empty() {
                return Err(SnsError::BadRequest("AttributeName is required".to_string()));
            }
            if !is_valid_subscription_attribute(&payload.attribute_name) {
                return Err(SnsError::BadRequest(format!(
                    "Unsupported attribute name: {}",
                    payload.attribute_name
                )));
            }
            validate_json_attribute(&payload.attribute_name, &payload.attribute_value)?;
            if payload.attribute_name == "RawMessageDelivery" {
                validate_raw_message_delivery(&payload.attribute_value)?;
            }
            Ok(())
        },
        handler: handle_set_subscription_attributes,
        span: "sns.subscription_attributes.set",
        span_attrs: |_request, payload, _store| {
            HashMap::from([
                ("subscription_arn".to_string(), payload.subscription_arn.clone()),
                ("attribute_name".to_string(), payload.attribute_name.clone()),
                ("attribute_value".to_string(), payload.attribute_value.clone()),
            ])
        },
        authorize_action: "sns:SetSubscriptionAttributes",
        authorize_with: crate::auth::resolve_authorization,
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct SetSubscriptionAttributesRequest {
    subscription_arn: String,
    attribute_name: String,
    attribute_value: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename = "SetSubscriptionAttributesResponse")]
#[serde(rename_all = "PascalCase")]
pub(crate) struct SetSubscriptionAttributesResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    response_metadata: ResponseMetadata,
}

async fn handle_set_subscription_attributes<S: SnsStore>(
    request: &ResolvedRequest,
    store: &S,
    request_body: SetSubscriptionAttributesRequest,
) -> Result<SetSubscriptionAttributesResponse, SnsError> {
    let update = SnsSubscriptionAttributeUpdate::from_attribute_name_and_value(
        &request_body.attribute_name,
        &request_body.attribute_value,
    );

    store
        .set_subscription_attributes(&request_body.subscription_arn, update)
        .await
        .map_err(|error| match error {
            hiraeth_store::StoreError::NotFound(_) => SnsError::SubscriptionNotFound,
            other => SnsError::from(other),
        })?;

    Ok(SetSubscriptionAttributesResponse {
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

    use super::{
        SetSubscriptionAttributesAction, SetSubscriptionAttributesResponse,
        handle_set_subscription_attributes,
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
            <SetSubscriptionAttributesAction as TypedAwsAction<SnsTestStore>>::name(
                &SetSubscriptionAttributesAction
            ),
            "SetSubscriptionAttributes"
        );
    }

    #[tokio::test]
    async fn updates_subscription_attribute() {
        let subscription_arn = "arn:aws:sns:us-east-1:123456789012:test-topic:sub-1";
        let store = SnsTestStore::with_subscription(subscription(subscription_arn));
        let request = resolved_request(&format!(
            "SubscriptionArn={subscription_arn}&AttributeName=RawMessageDelivery&AttributeValue=true"
        ));
        let body = crate::actions::test_support::parse_request_body(&request);

        handle_set_subscription_attributes(&request, &store, body)
            .await
            .expect("set subscription attributes should succeed");

        let subscription = store
            .get_subscription(subscription_arn)
            .await
            .expect("get subscription should succeed")
            .expect("subscription should exist");
        assert_eq!(subscription.raw_message_delivery, Some("true".to_string()));
    }

    #[tokio::test]
    async fn rejects_invalid_json_attribute() {
        let subscription_arn = "arn:aws:sns:us-east-1:123456789012:test-topic:sub-1";
        let store = SnsTestStore::with_subscription(subscription(subscription_arn));
        let request = resolved_request(&format!(
            "SubscriptionArn={subscription_arn}&AttributeName=FilterPolicy&AttributeValue=%7B"
        ));
        let body = crate::actions::test_support::parse_request_body(&request);

        let result = SetSubscriptionAttributesAction
            .validate(&request, &body, &store)
            .await;

        assert!(matches!(result, Err(SnsError::BadRequest(_))));
    }

    #[test]
    fn response_serializes_expected_xml_shape() {
        let response = SetSubscriptionAttributesResponse {
            xmlns: SNS_XMLNS,
            response_metadata: ResponseMetadata {
                request_id: "test-request-id".to_string(),
            },
        };

        let xml = String::from_utf8(xml_body(&response).unwrap()).unwrap();

        assert!(xml.contains("<SetSubscriptionAttributesResponse"));
        assert!(xml.contains("<ResponseMetadata>"));
        assert!(xml.contains("<RequestId>test-request-id</RequestId>"));
    }
}
