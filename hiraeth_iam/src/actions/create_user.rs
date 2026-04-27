use async_trait::async_trait;
use chrono::Utc;
use hiraeth_core::{
    AwsActionPayloadParseError, ResolvedRequest, ServiceResponse, TypedAwsAction,
    auth::AuthorizationCheck,
};
use hiraeth_store::{IamStore, iam::NewPrincipal};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    actions::util::{
        IAM_XMLNS, IamUserXml, ResponseMetadata, default_user_path, iam_xml_response, new_id,
        new_request_id, normalize_user_path, parse_payload_error, response_metadata, user_arn,
    },
    error::IamError,
};

pub(crate) struct CreateUserAction;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct CreateUserRequest {
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

#[async_trait]
impl<S> TypedAwsAction<S> for CreateUserAction
where
    S: IamStore + Send + Sync,
{
    type Request = CreateUserRequest;
    type Error = IamError;

    fn name(&self) -> &'static str {
        "CreateUser"
    }

    fn parse_error(&self, error: AwsActionPayloadParseError) -> IamError {
        parse_payload_error(error)
    }

    async fn handle_typed(
        &self,
        request: ResolvedRequest,
        create_user_request: CreateUserRequest,
        store: &S,
    ) -> Result<ServiceResponse, IamError> {
        let account_id = &request.auth_context.principal.account_id;

        let path = normalize_user_path(&create_user_request.path);
        let created_principal = store
            .create_principal(NewPrincipal {
                account_id: account_id.clone(),
                kind: "user".to_string(),
                name: create_user_request.user_name,
                path,
                user_id: new_id(),
            })
            .await?;

        let user_xml = created_principal.into();
        iam_xml_response(&create_user_response(user_xml, new_request_id()))
    }

    async fn resolve_authorization_typed(
        &self,
        request: &ResolvedRequest,
        create_user_request: CreateUserRequest,
        _store: &S,
    ) -> Result<AuthorizationCheck, IamError> {
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

fn create_user_response(user: IamUserXml, request_id: impl Into<String>) -> CreateUserResponse {
    CreateUserResponse {
        xmlns: IAM_XMLNS,
        result: CreateUserResult { user },
        response_metadata: response_metadata(request_id),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{NaiveDate, TimeZone, Utc};
    use hiraeth_core::{AuthContext, AwsAction, ResolvedRequest, TypedAwsActionAdapter, xml_body};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::{
        IamStore,
        iam::{AccessKey, InMemoryIamStore, Principal},
    };

    use crate::actions::util::new_id;

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
                path: "/".to_string(),
                user_id: "AIDATESTUSER000001".to_string(),
                created_at: Utc
                    .with_ymd_and_hms(2026, 4, 22, 12, 0, 0)
                    .unwrap()
                    .naive_utc(),
            }],
            [],
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
                    path: "/".to_string(),
                    user_id: "AIDATESTUSER000001".to_string(),
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
        let action = TypedAwsActionAdapter::new(CreateUserAction);
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
        let action = TypedAwsActionAdapter::new(CreateUserAction);
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
    async fn handle_returns_created_user_xml_response() {
        let action = TypedAwsActionAdapter::new(CreateUserAction);
        let response = action
            .handle(
                resolved_request(
                    b"Action=CreateUser&Version=2010-05-08&UserName=alice&Path=%2Fengineering%2Fdev%2F",
                ),
                &store(),
            )
            .await;

        let body = String::from_utf8(response.body).expect("response body should be utf-8");

        assert_eq!(response.status_code, 200);
        assert_eq!(
            response.headers,
            vec![(
                "content-type".to_string(),
                "text/xml; charset=utf-8".to_string()
            )]
        );
        assert!(body.contains("<UserName>alice</UserName>"));
        assert!(body.contains("<Path>/engineering/dev/</Path>"));
        assert!(body.contains("<Arn>arn:aws:iam::123456789012:user/engineering/dev/alice</Arn>"));
        assert!(body.contains("<UserId>AIDA"));
        assert!(body.contains("<ResponseMetadata><RequestId>"));
    }

    #[test]
    fn iam_user_xml_uses_principal_metadata() {
        let principal = Principal {
            id: 42,
            account_id: "123456789012".to_string(),
            kind: "user".to_string(),
            name: "Bob".to_string(),
            path: "/division_abc/subdivision_xyz/".to_string(),
            user_id: "AIDACKCEVSQ6C2EXAMPLE".to_string(),
            created_at: NaiveDate::from_ymd_opt(2026, 4, 23)
                .unwrap()
                .and_hms_opt(18, 20, 17)
                .unwrap(),
        };

        let user = IamUserXml::from(principal);

        assert_eq!(user.path, "/division_abc/subdivision_xyz/");
        assert_eq!(user.user_name, "Bob");
        assert_eq!(user.user_id, "AIDACKCEVSQ6C2EXAMPLE");
        assert_eq!(
            user.arn,
            "arn:aws:iam::123456789012:user/division_abc/subdivision_xyz/Bob"
        );
        assert_eq!(user.create_date, "2026-04-23T18:20:17Z");
    }

    #[test]
    fn new_user_id_uses_aida_prefix() {
        let user_id = new_id();

        assert!(user_id.starts_with("AIDA"));
        assert_eq!(user_id.len(), 36);
        assert!(
            user_id
                .chars()
                .all(|character| character.is_ascii_uppercase() || character.is_ascii_digit())
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
