use async_trait::async_trait;
use hiraeth_core::{
    ApiError, AwsAction, ResolvedRequest, ServiceResponse, auth::AuthorizationCheck,
    parse_aws_query_request, xml_response,
};
use hiraeth_store::{IamStore, iam::Principal};
use serde::{Deserialize, Serialize};

use crate::error::IamError;

pub(crate) struct CreateUserAction;
const IAM_XMLNS: &str = "https://iam.amazonaws.com/doc/2010-05-08/";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CreateUserRequest {
    user_name: String,
    #[serde(default = "default_user_path")]
    path: String,
    permissions_boundary: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename = "CreateUserResponse")]
struct CreateUserResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    #[serde(rename = "CreateUserResult")]
    result: CreateUserResult,
    #[serde(rename = "ResponseMetadata")]
    response_metadata: ResponseMetadata,
}

#[derive(Debug, Serialize)]
struct CreateUserResult {
    #[serde(rename = "User")]
    user: IamUserXml,
}

#[derive(Debug, Serialize)]
struct IamUserXml {
    #[serde(rename = "Path")]
    path: String,
    #[serde(rename = "UserName")]
    user_name: String,
    #[serde(rename = "UserId")]
    user_id: String,
    #[serde(rename = "Arn")]
    arn: String,
    #[serde(rename = "CreateDate")]
    create_date: String,
}

#[derive(Debug, Serialize)]
struct ResponseMetadata {
    #[serde(rename = "RequestId")]
    request_id: String,
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
        store: &S,
    ) -> Result<ServiceResponse, ApiError> {
        let account_id = &request.auth_context.principal.account_id;
        let create_user_request: CreateUserRequest = match parse_aws_query_request(&request.request)
        {
            Ok(request) => request,
            Err(error) => return Ok(ServiceResponse::from(IamError::from(error))),
        };

        let principal = Principal {
            id: 1,
            account_id: account_id.clone(),
            kind: "user".to_string(),
            name: create_user_request.user_name.clone(),
            created_at: chrono::Utc::now().naive_utc(),
        };

        let result = store
            .create_principal(principal)
            .await
            .map(|_| {
                create_user_response(
                    IamUserXml {
                        path: "/".to_string(),
                        user_name: create_user_request.user_name.clone(),
                        user_id: account_id.clone(),
                        arn: user_arn(account_id, "/", &create_user_request.user_name),
                        create_date: chrono::Utc::now().to_rfc3339(),
                    },
                    "bogus",
                )
            })
            .map(|result| xml_response(&result).map_err(IamError::from))
            .map_err(IamError::from)
            .map_err(ServiceResponse::from);

        match result {
            Ok(response) => match response {
                Ok(xml_response) => Ok(xml_response),
                Err(error) => Ok(ServiceResponse::from(error)),
            },
            Err(error) => Ok(error),
        }
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

fn create_user_response(user: IamUserXml, request_id: impl Into<String>) -> CreateUserResponse {
    CreateUserResponse {
        xmlns: IAM_XMLNS,
        result: CreateUserResult { user },
        response_metadata: ResponseMetadata {
            request_id: request_id.into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_core::{AuthContext, AwsAction, ResolvedRequest, xml_body};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::{
        IamStore,
        iam::{AccessKey, InMemoryIamStore, Principal},
    };

    use super::{CreateUserAction, IamUserXml, create_user_response};

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

    #[test]
    fn create_user_response_serializes_expected_xml_shape() {
        let xml = xml_body(&create_user_response(
            IamUserXml {
                path: "/division_abc/subdivision_xyz/".to_string(),
                user_name: "Bob".to_string(),
                user_id: "AIDACKCEVSQ6C2EXAMPLE".to_string(),
                arn: "arn:aws:iam::123456789012:user/division_abc/subdivision_xyz/Bob".to_string(),
                create_date: "2026-04-23T18:20:17Z".to_string(),
            },
            "7a62c49f-347e-4fc4-9331-6e8eEXAMPLE",
        ))
        .expect("create user response should serialize");

        assert_eq!(
            String::from_utf8(xml).unwrap(),
            concat!(
                r#"<CreateUserResponse xmlns="https://iam.amazonaws.com/doc/2010-05-08/">"#,
                r#"<CreateUserResult><User>"#,
                r#"<Path>/division_abc/subdivision_xyz/</Path>"#,
                r#"<UserName>Bob</UserName>"#,
                r#"<UserId>AIDACKCEVSQ6C2EXAMPLE</UserId>"#,
                r#"<Arn>arn:aws:iam::123456789012:user/division_abc/subdivision_xyz/Bob</Arn>"#,
                r#"<CreateDate>2026-04-23T18:20:17Z</CreateDate>"#,
                r#"</User></CreateUserResult>"#,
                r#"<ResponseMetadata>"#,
                r#"<RequestId>7a62c49f-347e-4fc4-9331-6e8eEXAMPLE</RequestId>"#,
                r#"</ResponseMetadata></CreateUserResponse>"#
            )
        );
    }
}
