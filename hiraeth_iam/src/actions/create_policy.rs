use async_trait::async_trait;
use hiraeth_core::{
    AwsActionPayloadParseError, ResolvedRequest, ServiceResponse, TypedAwsAction,
    auth::AuthorizationCheck,
};
use hiraeth_store::{IamStore, iam::NewManagedPolicy};
use serde::{Deserialize, Serialize};

use crate::{
    actions::util::{
        IAM_XMLNS, IamPolicyXml, ResponseMetadata, iam_xml_response, new_id, normalize_policy_path,
        parse_payload_error, response_metadata,
    },
    error::IamError,
};

pub(crate) struct CreatePolicyAction;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct CreatePolicyRequest {
    path: Option<String>,
    policy_document: String,
    policy_name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
struct CreatePolicyResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    create_policy_result: CreatePolicyResult,
    response_metadata: ResponseMetadata,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
struct CreatePolicyResult {
    policy: IamPolicyXml,
}

#[async_trait]
impl<S> TypedAwsAction<S> for CreatePolicyAction
where
    S: IamStore + Send + Sync,
{
    type Request = CreatePolicyRequest;
    type Error = IamError;

    fn name(&self) -> &'static str {
        "CreatePolicy"
    }

    fn parse_error(&self, error: AwsActionPayloadParseError) -> IamError {
        parse_payload_error(error)
    }

    async fn handle_typed(
        &self,
        request: ResolvedRequest,
        create_policy_request: CreatePolicyRequest,
        store: &S,
    ) -> Result<ServiceResponse, IamError> {
        let account_id = request.auth_context.principal.account_id.clone();
        let policy_path = normalize_policy_path(create_policy_request.path.as_deref());
        let created_policy = store
            .insert_managed_policy(NewManagedPolicy {
                account_id,
                policy_id: new_id(),
                policy_name: create_policy_request.policy_name.clone(),
                policy_path: Some(policy_path),
                policy_document: create_policy_request.policy_document.clone(),
            })
            .await?;

        iam_xml_response(&CreatePolicyResponse {
            xmlns: IAM_XMLNS,
            create_policy_result: CreatePolicyResult {
                policy: created_policy.into(),
            },
            response_metadata: response_metadata(request.request_id),
        })
    }

    async fn resolve_authorization_typed(
        &self,
        request: &ResolvedRequest,
        create_policy_request: CreatePolicyRequest,
        _store: &S,
    ) -> Result<AuthorizationCheck, IamError> {
        let policy_path = normalize_policy_path(create_policy_request.path.as_deref());
        Ok(AuthorizationCheck {
            action: "iam:CreatePolicy".to_string(),
            resource: format!(
                "arn:aws:iam::{}:policy{}{}",
                request.auth_context.principal.account_id,
                policy_path,
                create_policy_request.policy_name
            ),
            resource_policy: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_core::{AuthContext, AwsAction, TypedAwsActionAdapter, xml_body};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::iam::{AccessKey, InMemoryIamStore, ManagedPolicyStore, Principal};

    use super::CreatePolicyAction;

    fn principal(id: i64, name: &str, path: &str) -> Principal {
        Principal {
            id,
            account_id: "123456789012".to_string(),
            kind: "user".to_string(),
            name: name.to_string(),
            path: path.to_string(),
            user_id: format!("AIDATESTUSER{id:08}"),
            created_at: Utc
                .with_ymd_and_hms(2026, 4, 28, 12, 0, 0)
                .unwrap()
                .naive_utc(),
        }
    }

    fn store() -> InMemoryIamStore {
        InMemoryIamStore::new(
            [AccessKey {
                key_id: "AKIAIOSFODNN7EXAMPLE".to_string(),
                principal_id: 1,
                secret_key: "secret".to_string(),
                created_at: Utc
                    .with_ymd_and_hms(2026, 4, 28, 12, 0, 0)
                    .unwrap()
                    .naive_utc(),
            }],
            [principal(1, "signing-user", "/")],
            [],
            [],
            [],
        )
    }

    fn resolved_request(body: &[u8]) -> hiraeth_core::ResolvedRequest {
        hiraeth_core::ResolvedRequest {
            request_id: "test-request-id".to_string(),
            request: IncomingRequest {
                host: "iam.amazonaws.com".to_string(),
                method: "POST".to_string(),
                path: "/".to_string(),
                query: None,
                headers: [(
                    "content-type".to_string(),
                    "application/x-www-form-urlencoded".to_string(),
                )]
                .into_iter()
                .collect::<HashMap<_, _>>(),
                body: body.to_vec(),
            },
            service: "iam".to_string(),
            region: "us-east-1".to_string(),
            auth_context: AuthContext {
                access_key: "AKIAIOSFODNN7EXAMPLE".to_string(),
                principal: principal(1, "signing-user", "/"),
            },
            date: Utc.with_ymd_and_hms(2026, 4, 28, 12, 0, 0).unwrap(),
        }
    }

    #[tokio::test]
    async fn handle_creates_policy_with_normalized_path() {
        let action = TypedAwsActionAdapter::new(CreatePolicyAction);
        let store = store();
        let response = action
            .handle(
                resolved_request(
                    b"Action=CreatePolicy&Version=2010-05-08&PolicyName=orders-readonly&Path=dev/team-a&PolicyDocument=%7B%22Version%22%3A%222012-10-17%22%2C%22Statement%22%3A%5B%7B%22Effect%22%3A%22Allow%22%2C%22Action%22%3A%22sqs%3AReceiveMessage%22%2C%22Resource%22%3A%22*%22%7D%5D%7D",
                ),
                &store,
            )
            .await;

        let created_policy = store
            .get_managed_policy("123456789012", "orders-readonly", "/dev/team-a/")
            .await
            .expect("managed policy lookup should succeed")
            .expect("managed policy should exist");

        assert_eq!(response.status_code, 200);
        let body = String::from_utf8(response.body).expect("response should be utf8");
        assert!(body.contains("<CreatePolicyResponse"));
        assert!(body.contains("<PolicyName>orders-readonly</PolicyName>"));
        assert!(body.contains("<Path>/dev/team-a/</Path>"));
        assert_eq!(created_policy.policy_path.as_deref(), Some("/dev/team-a/"));
    }

    #[tokio::test]
    async fn resolve_authorization_uses_policy_path_in_resource_arn() {
        let action = TypedAwsActionAdapter::new(CreatePolicyAction);
        let check = action
            .resolve_authorization(
                &resolved_request(
                    b"Action=CreatePolicy&Version=2010-05-08&PolicyName=orders-readonly&Path=dev/team-a&PolicyDocument=%7B%22Version%22%3A%222012-10-17%22%2C%22Statement%22%3A%5B%5D%7D",
                ),
                &store(),
            )
            .await
            .expect("authorization check should resolve");

        assert_eq!(check.action, "iam:CreatePolicy");
        assert_eq!(
            check.resource,
            "arn:aws:iam::123456789012:policy/dev/team-a/orders-readonly"
        );
    }

    #[test]
    fn create_policy_response_serializes_expected_xml_shape() {
        let response = super::CreatePolicyResponse {
            xmlns: super::IAM_XMLNS,
            create_policy_result: super::CreatePolicyResult {
                policy: super::IamPolicyXml {
                    path: Some("/".to_string()),
                    policy_name: Some("orders-readonly".to_string()),
                    default_version_id: None,
                    policy_id: Some("AIDAEXAMPLE".to_string()),
                    arn: Some("arn:aws:iam::123456789012:policy/orders-readonly".to_string()),
                    attachments_count: None,
                    create_date: None,
                    update_date: None,
                },
            },
            response_metadata: super::response_metadata("request-id"),
        };

        let xml = String::from_utf8(xml_body(&response).expect("xml should serialize"))
            .expect("xml should be utf8");
        assert!(xml.contains(
            r#"<CreatePolicyResponse xmlns="https://iam.amazonaws.com/doc/2010-05-08/">"#
        ));
        assert!(xml.contains("<CreatePolicyResult>"));
        assert!(xml.contains("<PolicyName>orders-readonly</PolicyName>"));
        assert!(
            xml.contains("<ResponseMetadata><RequestId>request-id</RequestId></ResponseMetadata>")
        );
    }
}
