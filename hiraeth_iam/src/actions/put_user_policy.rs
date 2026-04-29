use async_trait::async_trait;
use hiraeth_core::{
    AwsActionPayloadParseError, ResolvedRequest, ServiceResponse, TypedAwsAction, arn_util,
    auth::AuthorizationCheck,
};
use hiraeth_store::IamStore;
use serde::{Deserialize, Serialize};

use crate::{
    actions::util::{
        self, ResponseMetadata, iam_xml_response, parse_payload_error, response_metadata,
    },
    error::IamError,
};

pub(crate) struct PutUserPolicyAction;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct PutUserPolicyRequest {
    pub user_name: String,
    pub policy_name: String,
    pub policy_document: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
struct PutUserPolicyResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    response_metadata: ResponseMetadata,
}

#[async_trait]
impl<S> TypedAwsAction<S> for PutUserPolicyAction
where
    S: IamStore + Send + Sync,
{
    type Request = PutUserPolicyRequest;
    type Error = IamError;

    fn name(&self) -> &'static str {
        "PutUserPolicy"
    }

    fn parse_error(&self, error: AwsActionPayloadParseError) -> Self::Error {
        parse_payload_error(error)
    }

    async fn handle(
        &self,
        request: ResolvedRequest,
        put_policy_request: Self::Request,
        store: &S,
    ) -> Result<ServiceResponse, Self::Error> {
        let account_id = &request.auth_context.principal.account_id;
        let user = store
            .get_principal_by_identity(&account_id, "user", &put_policy_request.user_name)
            .await?
            .ok_or_else(|| {
                IamError::NoSuchEntity(format!(
                    "User {} does not exist",
                    put_policy_request.user_name
                ))
            })?;

        store
            .put_inline_policy(
                user.id,
                &put_policy_request.policy_name,
                &put_policy_request.policy_document,
            )
            .await?;

        let response = PutUserPolicyResponse {
            xmlns: util::IAM_XMLNS,
            response_metadata: response_metadata(request.request_id),
        };

        iam_xml_response(&response)
    }

    async fn resolve_authorization_typed(
        &self,
        request: &ResolvedRequest,
        put_policy_request: PutUserPolicyRequest,
        store: &S,
    ) -> Result<AuthorizationCheck, Self::Error> {
        let account_id = &request.auth_context.principal.account_id;
        let user = store
            .get_principal_by_identity(&account_id, "user", &put_policy_request.user_name)
            .await?
            .ok_or_else(|| {
                IamError::NoSuchEntity(format!(
                    "User {} does not exist",
                    put_policy_request.user_name
                ))
            })?;

        let arn = arn_util::user_arn(account_id, &user.path, &user.name);

        Ok(AuthorizationCheck {
            action: "iam:PutUserPolicy".to_string(),
            resource: arn,
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
    use hiraeth_store::iam::{AccessKey, InMemoryIamStore, Principal, PrincipalInlinePolicyStore};

    use super::PutUserPolicyAction;

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
            [
                principal(1, "signing-user", "/"),
                principal(2, "alice", "/engineering/"),
            ],
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
    async fn handle_puts_inline_policy_for_target_user() {
        let action = TypedAwsActionAdapter::new(PutUserPolicyAction);
        let store = store();
        let response = action
            .handle(
                resolved_request(
                    b"Action=PutUserPolicy&Version=2010-05-08&UserName=alice&PolicyName=sqs-read&PolicyDocument=%7B%22Version%22%3A%222012-10-17%22%2C%22Statement%22%3A%5B%7B%22Effect%22%3A%22Allow%22%2C%22Action%22%3A%22sqs%3AReceiveMessage%22%2C%22Resource%22%3A%22*%22%7D%5D%7D",
                ),
                &store,
            )
            .await;

        let policies = store
            .get_inline_policies_for_principal(2)
            .await
            .expect("inline policy lookup should succeed");

        assert_eq!(response.status_code, 200);
        assert_eq!(policies.len(), 1);
        assert_eq!(policies[0].policy_name, "sqs-read");
    }

    #[tokio::test]
    async fn handle_returns_no_such_entity_for_unknown_user() {
        let action = TypedAwsActionAdapter::new(PutUserPolicyAction);
        let response = action
            .handle(
                resolved_request(
                    b"Action=PutUserPolicy&Version=2010-05-08&UserName=missing&PolicyName=sqs-read&PolicyDocument=%7B%22Version%22%3A%222012-10-17%22%7D",
                ),
                &store(),
            )
            .await;

        assert_eq!(response.status_code, 404);
        let body = String::from_utf8(response.body).expect("response should be utf8");
        assert!(body.contains("<Code>NoSuchEntity</Code>"));
    }

    #[tokio::test]
    async fn resolve_authorization_targets_user_arn() {
        let action = TypedAwsActionAdapter::new(PutUserPolicyAction);
        let check = action
            .resolve_authorization(
                &resolved_request(
                    b"Action=PutUserPolicy&Version=2010-05-08&UserName=alice&PolicyName=sqs-read&PolicyDocument=%7B%22Version%22%3A%222012-10-17%22%7D",
                ),
                &store(),
            )
            .await
            .expect("authz should resolve");

        assert_eq!(check.action, "iam:PutUserPolicy");
        assert_eq!(
            check.resource,
            "arn:aws:iam::123456789012:user/engineering/alice"
        );
    }

    #[test]
    fn put_user_policy_response_serializes_expected_xml_shape() {
        let response = super::PutUserPolicyResponse {
            xmlns: super::util::IAM_XMLNS,
            response_metadata: super::response_metadata("request-id"),
        };

        let xml = String::from_utf8(xml_body(&response).expect("xml should serialize"))
            .expect("xml should be utf8");
        assert!(xml.contains(
            r#"<PutUserPolicyResponse xmlns="https://iam.amazonaws.com/doc/2010-05-08/">"#
        ));
        assert!(
            xml.contains("<ResponseMetadata><RequestId>request-id</RequestId></ResponseMetadata>")
        );
    }
}
