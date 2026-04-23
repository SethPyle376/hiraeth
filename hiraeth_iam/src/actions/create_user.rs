use async_trait::async_trait;
use hiraeth_core::{
    ApiError, AwsAction, ResolvedRequest, ServiceResponse, auth::AuthorizationCheck,
    parse_aws_query_request,
};
use hiraeth_store::IamStore;
use serde::Deserialize;

use crate::error::IamError;

pub(crate) struct CreateUserAction;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CreateUserRequest {
    user_name: String,
    #[serde(default = "default_user_path")]
    path: String,
    permissions_boundary: Option<String>,
}

#[async_trait]
impl<S> AwsAction<S> for CreateUserAction
where
    S: IamStore + Send + Sync,
{
    fn name(&self) -> &'static str {
        "CreateUser"
    }

    async fn handle(
        &self,
        request: ResolvedRequest,
        _store: &S,
    ) -> Result<ServiceResponse, ApiError> {
        let create_user_request: CreateUserRequest = match parse_aws_query_request(&request.request)
        {
            Ok(request) => request,
            Err(error) => return Ok(ServiceResponse::from(IamError::from(error))),
        };
        let _ = (
            create_user_request.user_name,
            create_user_request.path,
            create_user_request.permissions_boundary,
        );

        Ok(ServiceResponse::from(IamError::UnsupportedOperation(
            "CreateUser".to_string(),
        )))
    }

    async fn resolve_authorization(
        &self,
        request: &ResolvedRequest,
        _store: &S,
    ) -> Result<AuthorizationCheck, ServiceResponse> {
        let create_user_request: CreateUserRequest = parse_aws_query_request(&request.request)
            .map_err(IamError::from)
            .map_err(ServiceResponse::from)?;

        Ok(AuthorizationCheck {
            action: "iam:CreateUser".to_string(),
            resource: user_arn(
                &request.auth_context.principal.account_id,
                &create_user_request.path,
                &create_user_request.user_name,
            ),
            resource_policy: None,
        })
    }
}

fn user_arn(account_id: &str, path: &str, user_name: &str) -> String {
    format!(
        "arn:aws:iam::{account_id}:user{}{user_name}",
        normalize_user_path(path)
    )
}

fn normalize_user_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed == "/" {
        "/".to_string()
    } else {
        let trimmed = trimmed.trim_matches('/');
        format!("/{trimmed}/")
    }
}

fn default_user_path() -> String {
    "/".to_string()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_core::{AuthContext, AwsAction, ResolvedRequest};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::{
        IamStore,
        iam::{AccessKey, InMemoryIamStore, Principal},
    };

    use super::CreateUserAction;

    fn store() -> InMemoryIamStore {
        InMemoryIamStore::new(
            [AccessKey {
                key_id: "AKIAIOSFODNN7EXAMPLE".to_string(),
                principal_id: 1,
                secret_key: "secret".to_string(),
                created_at: Utc
                    .with_ymd_and_hms(2026, 4, 22, 12, 0, 0)
                    .unwrap()
                    .naive_utc(),
            }],
            [Principal {
                id: 1,
                account_id: "123456789012".to_string(),
                kind: "user".to_string(),
                name: "test-user".to_string(),
                created_at: Utc
                    .with_ymd_and_hms(2026, 4, 22, 12, 0, 0)
                    .unwrap()
                    .naive_utc(),
            }],
            [],
        )
    }

    fn resolved_request(body: &[u8]) -> ResolvedRequest {
        ResolvedRequest {
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
                principal: Principal {
                    id: 1,
                    account_id: "123456789012".to_string(),
                    kind: "user".to_string(),
                    name: "test-user".to_string(),
                    created_at: Utc
                        .with_ymd_and_hms(2026, 4, 22, 12, 0, 0)
                        .unwrap()
                        .naive_utc(),
                },
            },
            date: Utc.with_ymd_and_hms(2026, 4, 22, 12, 0, 0).unwrap(),
        }
    }

    #[tokio::test]
    async fn resolve_authorization_uses_default_user_path() {
        let action = CreateUserAction;
        let check = action
            .resolve_authorization(
                &resolved_request(b"Action=CreateUser&Version=2010-05-08&UserName=alice"),
                &store(),
            )
            .await
            .expect("auth check should resolve");

        assert_eq!(check.action, "iam:CreateUser");
        assert_eq!(check.resource, "arn:aws:iam::123456789012:user/alice");
        assert!(check.resource_policy.is_none());
    }

    #[tokio::test]
    async fn resolve_authorization_uses_custom_user_path() {
        let action = CreateUserAction;
        let check = action
            .resolve_authorization(
                &resolved_request(
                    b"Action=CreateUser&Version=2010-05-08&UserName=alice&Path=%2Fengineering%2Fdev%2F",
                ),
                &store(),
            )
            .await
            .expect("auth check should resolve");

        assert_eq!(
            check.resource,
            "arn:aws:iam::123456789012:user/engineering/dev/alice"
        );
    }

    #[tokio::test]
    async fn handle_returns_not_implemented_placeholder() {
        let action = CreateUserAction;
        let response = action
            .handle(
                resolved_request(b"Action=CreateUser&Version=2010-05-08&UserName=alice"),
                &store(),
            )
            .await
            .expect("placeholder response should be returned");

        assert_eq!(response.status_code, 501);
        assert_eq!(
            String::from_utf8(response.body).unwrap(),
            "IAM action CreateUser is not implemented"
        );
    }
}
