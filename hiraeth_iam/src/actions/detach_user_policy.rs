use async_trait::async_trait;
use hiraeth_core::{
    AwsActionPayloadParseError, ResolvedRequest, ServiceResponse, TypedAwsAction, arn_util,
    auth::AuthorizationCheck,
};
use hiraeth_store::IamStore;
use serde::{Deserialize, Serialize};

use crate::{
    actions::util::{
        IAM_XMLNS, ResponseMetadata, iam_xml_response, parse_payload_error, parse_policy_arn,
        response_metadata,
    },
    error::IamError,
};

pub(crate) struct DetachUserPolicyAction;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct DetachUserPolicyRequest {
    pub user_name: String,
    pub policy_arn: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
struct DetachUserPolicyResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    response_metadata: ResponseMetadata,
}

#[async_trait]
impl<S> TypedAwsAction<S> for DetachUserPolicyAction
where
    S: IamStore + Send + Sync,
{
    type Request = DetachUserPolicyRequest;
    type Error = IamError;

    fn name(&self) -> &'static str {
        "DetachUserPolicy"
    }

    fn parse_error(&self, error: AwsActionPayloadParseError) -> Self::Error {
        parse_payload_error(error)
    }

    async fn handle_typed(
        &self,
        request: ResolvedRequest,
        detach_policy_request: DetachUserPolicyRequest,
        store: &S,
    ) -> Result<ServiceResponse, IamError> {
        let account_id = &request.auth_context.principal.account_id;
        let user = store
            .get_principal_by_identity(account_id, "user", &detach_policy_request.user_name)
            .await?
            .ok_or_else(|| {
                IamError::NoSuchEntity(format!("User {}", detach_policy_request.user_name))
            })?;

        let policy_arn = parse_policy_arn(&detach_policy_request.policy_arn)?;
        if policy_arn.account_id != *account_id {
            return Err(IamError::NoSuchEntity(format!(
                "Policy {} does not exist",
                detach_policy_request.policy_arn
            )));
        }
        let policy = store
            .get_managed_policy(
                &policy_arn.account_id,
                &policy_arn.policy_name,
                &policy_arn.policy_path,
            )
            .await?
            .ok_or_else(|| {
                IamError::NoSuchEntity(format!(
                    "Policy {} does not exist",
                    detach_policy_request.policy_arn
                ))
            })?;
        store
            .detach_policy_from_principal(policy.id, user.id)
            .await?;

        let response = DetachUserPolicyResponse {
            xmlns: IAM_XMLNS,
            response_metadata: response_metadata(request.request_id),
        };
        iam_xml_response(&response)
    }

    async fn resolve_authorization_typed(
        &self,
        request: &ResolvedRequest,
        attach_policy_request: DetachUserPolicyRequest,
        store: &S,
    ) -> Result<AuthorizationCheck, IamError> {
        let account_id = &request.auth_context.principal.account_id;
        let user = store
            .get_principal_by_identity(account_id, "user", &attach_policy_request.user_name)
            .await?
            .ok_or_else(|| {
                IamError::NoSuchEntity(format!("User {}", attach_policy_request.user_name))
            })?;

        Ok(AuthorizationCheck {
            action: "iam:DetachUserPolicy".to_string(),
            resource: arn_util::user_arn(account_id, &user.path, &user.name),
            resource_policy: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_core::{AuthContext, AwsAction, TypedAwsActionAdapter};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::iam::{
        AccessKey, InMemoryIamStore, ManagedPolicy, ManagedPolicyStore, Principal,
    };

    use super::DetachUserPolicyAction;

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

    fn managed_policy(id: i64, account: &str, name: &str, path: &str) -> ManagedPolicy {
        ManagedPolicy {
            id,
            policy_id: format!("AIDAPOLICY{id:08}"),
            account_id: account.to_string(),
            policy_name: name.to_string(),
            policy_path: Some(path.to_string()),
            policy_document: r#"{"Version":"2012-10-17","Statement":[]}"#.to_string(),
            created_at: Utc
                .with_ymd_and_hms(2026, 4, 28, 12, 0, 0)
                .unwrap()
                .naive_utc(),
            updated_at: Utc
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
                principal(2, "alice", "/"),
            ],
            [],
            [managed_policy(
                10,
                "123456789012",
                "orders-readonly",
                "/dev/",
            )],
            std::iter::empty(),
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
    async fn handle_detaches_attached_policy_from_user() {
        let action = TypedAwsActionAdapter::new(DetachUserPolicyAction);
        let store = store();
        store
            .attach_policy_to_principal(10, 2)
            .await
            .expect("setup attachment should succeed");
        let response = action
            .handle(
                resolved_request(
                    b"Action=DetachUserPolicy&Version=2010-05-08&UserName=alice&PolicyArn=arn%3Aaws%3Aiam%3A%3A123456789012%3Apolicy%2Fdev%2Forders-readonly",
                ),
                &store,
            )
            .await;

        let attached = store
            .get_managed_policies_attached_to_principal(2)
            .await
            .expect("attached policy lookup should succeed");

        assert_eq!(response.status_code, 200);
        assert!(attached.is_empty());
    }

    #[tokio::test]
    async fn handle_rejects_cross_account_policy_arn() {
        let action = TypedAwsActionAdapter::new(DetachUserPolicyAction);
        let response = action
            .handle(
                resolved_request(
                    b"Action=DetachUserPolicy&Version=2010-05-08&UserName=alice&PolicyArn=arn%3Aaws%3Aiam%3A%3A999999999999%3Apolicy%2Fdev%2Forders-readonly",
                ),
                &store(),
            )
            .await;

        assert_eq!(response.status_code, 404);
        let body = String::from_utf8(response.body).expect("response should be utf8");
        assert!(body.contains("<Code>NoSuchEntity</Code>"));
    }
}
