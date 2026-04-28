use async_trait::async_trait;
use chrono::SecondsFormat;
use hiraeth_core::{
    AwsActionPayloadParseError, ResolvedRequest, ServiceResponse, TypedAwsAction, arn_util,
    auth::AuthorizationCheck,
};
use hiraeth_store::{IamStore, iam::AccessKey};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    actions::util::{
        IAM_XMLNS, ResponseMetadata, iam_xml_response, parse_payload_error,
        requested_or_signing_user, response_metadata,
    },
    error::IamError,
};

pub(crate) struct CreateAccessKeyAction;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct CreateAccessKeyRequest {
    user_name: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename = "CreateAccessKeyResponse")]
struct CreateAccessKeyResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    #[serde(rename = "CreateAccessKeyResult")]
    result: CreateAccessKeyResult,
    #[serde(rename = "ResponseMetadata")]
    response_metadata: ResponseMetadata,
}

#[derive(Debug, Serialize)]
struct CreateAccessKeyResult {
    #[serde(rename = "AccessKey")]
    access_key: IamAccessKeyXml,
}

#[derive(Debug, Serialize)]
struct IamAccessKeyXml {
    #[serde(rename = "UserName")]
    user_name: String,
    #[serde(rename = "AccessKeyId")]
    access_key_id: String,
    #[serde(rename = "Status")]
    status: &'static str,
    #[serde(rename = "SecretAccessKey")]
    secret_access_key: String,
    #[serde(rename = "CreateDate")]
    create_date: String,
}

#[async_trait]
impl<S> TypedAwsAction<S> for CreateAccessKeyAction
where
    S: IamStore + Send + Sync,
{
    type Request = CreateAccessKeyRequest;
    type Error = IamError;

    fn name(&self) -> &'static str {
        "CreateAccessKey"
    }

    fn parse_error(&self, error: AwsActionPayloadParseError) -> IamError {
        parse_payload_error(error)
    }

    async fn handle_typed(
        &self,
        request: ResolvedRequest,
        create_access_key_request: CreateAccessKeyRequest,
        store: &S,
    ) -> Result<ServiceResponse, IamError> {
        let target_user = requested_or_signing_user(
            &request,
            store,
            create_access_key_request.user_name.as_deref(),
        )
        .await?;

        let access_key_id = new_access_key_id();
        let secret_access_key = new_secret_access_key();
        let created_access_key = store
            .insert_secret_key(&access_key_id, &secret_access_key, target_user.id)
            .await?;

        iam_xml_response(&create_access_key_response(
            iam_access_key_xml(&target_user.name, &created_access_key),
            request.request_id,
        ))
    }

    async fn resolve_authorization_typed(
        &self,
        request: &ResolvedRequest,
        create_access_key_request: CreateAccessKeyRequest,
        store: &S,
    ) -> Result<AuthorizationCheck, IamError> {
        let target_user = requested_or_signing_user(
            request,
            store,
            create_access_key_request.user_name.as_deref(),
        )
        .await?;

        Ok(AuthorizationCheck {
            action: "iam:CreateAccessKey".to_string(),
            resource: arn_util::user_arn(
                &target_user.account_id,
                &target_user.path,
                &target_user.name,
            ),
            resource_policy: None,
        })
    }
}

fn iam_access_key_xml(user_name: &str, access_key: &AccessKey) -> IamAccessKeyXml {
    IamAccessKeyXml {
        user_name: user_name.to_string(),
        access_key_id: access_key.key_id.clone(),
        status: "Active",
        secret_access_key: access_key.secret_key.clone(),
        create_date: access_key
            .created_at
            .and_utc()
            .to_rfc3339_opts(SecondsFormat::Secs, true),
    }
}

fn create_access_key_response(
    access_key: IamAccessKeyXml,
    request_id: impl Into<String>,
) -> CreateAccessKeyResponse {
    CreateAccessKeyResponse {
        xmlns: IAM_XMLNS,
        result: CreateAccessKeyResult { access_key },
        response_metadata: response_metadata(request_id),
    }
}

fn new_access_key_id() -> String {
    let random = Uuid::new_v4().simple().to_string().to_uppercase();
    format!("AKIA{}", &random[..16])
}

fn new_secret_access_key() -> String {
    let random = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    random[..40].to_string()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{NaiveDate, TimeZone, Utc};
    use hiraeth_core::{AuthContext, AwsAction, ResolvedRequest, TypedAwsActionAdapter, xml_body};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::iam::{
        AccessKey, AccessKeyStore, InMemoryIamStore, Principal, PrincipalStore,
    };

    use super::{
        CreateAccessKeyAction, IamAccessKeyXml, create_access_key_response, iam_access_key_xml,
        new_access_key_id, new_secret_access_key,
    };

    fn principal(id: i64, name: &str, path: &str) -> Principal {
        Principal {
            id,
            account_id: "123456789012".to_string(),
            kind: "user".to_string(),
            name: name.to_string(),
            path: path.to_string(),
            user_id: format!("AIDATESTUSER{id:08}"),
            created_at: Utc
                .with_ymd_and_hms(2026, 4, 22, 12, 0, 0)
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
                    .with_ymd_and_hms(2026, 4, 22, 12, 0, 0)
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

    fn resolved_request(body: &[u8]) -> ResolvedRequest {
        ResolvedRequest {
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
            date: Utc.with_ymd_and_hms(2026, 4, 22, 12, 0, 0).unwrap(),
        }
    }

    #[tokio::test]
    async fn resolve_authorization_uses_requested_user() {
        let action = TypedAwsActionAdapter::new(CreateAccessKeyAction);
        let check = action
            .resolve_authorization(
                &resolved_request(b"Action=CreateAccessKey&Version=2010-05-08&UserName=alice"),
                &store(),
            )
            .await
            .expect("auth check should resolve");

        assert_eq!(check.action, "iam:CreateAccessKey");
        assert_eq!(
            check.resource,
            "arn:aws:iam::123456789012:user/engineering/alice"
        );
        assert!(check.resource_policy.is_none());
    }

    #[tokio::test]
    async fn resolve_authorization_uses_signing_user_when_user_name_is_omitted() {
        let action = TypedAwsActionAdapter::new(CreateAccessKeyAction);
        let check = action
            .resolve_authorization(
                &resolved_request(b"Action=CreateAccessKey&Version=2010-05-08"),
                &store(),
            )
            .await
            .expect("auth check should resolve");

        assert_eq!(check.action, "iam:CreateAccessKey");
        assert_eq!(
            check.resource,
            "arn:aws:iam::123456789012:user/signing-user"
        );
        assert!(check.resource_policy.is_none());
    }

    #[tokio::test]
    async fn handle_creates_access_key_for_requested_user() {
        let action = TypedAwsActionAdapter::new(CreateAccessKeyAction);
        let store = store();
        let response = action
            .handle(
                resolved_request(b"Action=CreateAccessKey&Version=2010-05-08&UserName=alice"),
                &store,
            )
            .await;

        let body = String::from_utf8(response.body).expect("response body should be utf-8");
        let alice = store
            .get_principal_by_identity("123456789012", "user", "alice")
            .await
            .expect("principal lookup should succeed")
            .expect("alice should exist");
        let keys = store
            .list_access_keys_for_principal(alice.id)
            .await
            .expect("key listing should succeed");

        assert_eq!(response.status_code, 200);
        assert_eq!(keys.len(), 1);
        assert!(body.contains("<CreateAccessKeyResponse"));
        assert!(body.contains("<UserName>alice</UserName>"));
        assert!(body.contains("<AccessKeyId>AKIA"));
        assert!(body.contains("<Status>Active</Status>"));
        assert!(body.contains("<SecretAccessKey>"));
    }

    #[tokio::test]
    async fn handle_creates_access_key_for_signing_user_when_user_name_is_omitted() {
        let action = TypedAwsActionAdapter::new(CreateAccessKeyAction);
        let store = store();
        let response = action
            .handle(
                resolved_request(b"Action=CreateAccessKey&Version=2010-05-08"),
                &store,
            )
            .await;

        let body = String::from_utf8(response.body).expect("response body should be utf-8");
        let keys = store
            .list_access_keys_for_principal(1)
            .await
            .expect("key listing should succeed");

        assert_eq!(response.status_code, 200);
        assert_eq!(keys.len(), 2);
        assert!(body.contains("<UserName>signing-user</UserName>"));
    }

    #[tokio::test]
    async fn handle_returns_no_such_entity_for_missing_user() {
        let action = TypedAwsActionAdapter::new(CreateAccessKeyAction);
        let response = action
            .handle(
                resolved_request(b"Action=CreateAccessKey&Version=2010-05-08&UserName=missing"),
                &store(),
            )
            .await;

        let body = String::from_utf8(response.body).expect("response body should be utf-8");

        assert_eq!(response.status_code, 404);
        assert!(body.contains("<Code>NoSuchEntity</Code>"));
        assert!(body.contains("<Message>User missing does not exist</Message>"));
    }

    #[test]
    fn iam_access_key_xml_uses_access_key_metadata() {
        let access_key = AccessKey {
            key_id: "AKIACKCEVSQ6C2EXAMPLE".to_string(),
            principal_id: 42,
            secret_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".to_string(),
            created_at: NaiveDate::from_ymd_opt(2026, 4, 23)
                .unwrap()
                .and_hms_opt(18, 20, 17)
                .unwrap(),
        };

        let access_key_xml = iam_access_key_xml("Bob", &access_key);

        assert_eq!(access_key_xml.user_name, "Bob");
        assert_eq!(access_key_xml.access_key_id, "AKIACKCEVSQ6C2EXAMPLE");
        assert_eq!(access_key_xml.status, "Active");
        assert_eq!(
            access_key_xml.secret_access_key,
            "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"
        );
        assert_eq!(access_key_xml.create_date, "2026-04-23T18:20:17Z");
    }

    #[test]
    fn new_access_key_id_uses_akia_prefix() {
        let access_key_id = new_access_key_id();

        assert!(access_key_id.starts_with("AKIA"));
        assert_eq!(access_key_id.len(), 20);
        assert!(
            access_key_id
                .chars()
                .all(|character| character.is_ascii_uppercase() || character.is_ascii_digit())
        );
    }

    #[test]
    fn new_secret_access_key_uses_expected_length() {
        let secret_access_key = new_secret_access_key();

        assert_eq!(secret_access_key.len(), 40);
        assert!(
            secret_access_key
                .chars()
                .all(|character| character.is_ascii_hexdigit())
        );
    }

    #[test]
    fn create_access_key_response_serializes_expected_xml_shape() {
        let xml = xml_body(&create_access_key_response(
            IamAccessKeyXml {
                user_name: "Bob".to_string(),
                access_key_id: "AKIACKCEVSQ6C2EXAMPLE".to_string(),
                status: "Active",
                secret_access_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".to_string(),
                create_date: "2026-04-23T18:20:17Z".to_string(),
            },
            "7a62c49f-347e-4fc4-9331-6e8eEXAMPLE",
        ))
        .expect("create access key response should serialize");

        assert_eq!(
            String::from_utf8(xml).unwrap(),
            concat!(
                r#"<CreateAccessKeyResponse xmlns="https://iam.amazonaws.com/doc/2010-05-08/">"#,
                r#"<CreateAccessKeyResult><AccessKey>"#,
                r#"<UserName>Bob</UserName>"#,
                r#"<AccessKeyId>AKIACKCEVSQ6C2EXAMPLE</AccessKeyId>"#,
                r#"<Status>Active</Status>"#,
                r#"<SecretAccessKey>wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY</SecretAccessKey>"#,
                r#"<CreateDate>2026-04-23T18:20:17Z</CreateDate>"#,
                r#"</AccessKey></CreateAccessKeyResult>"#,
                r#"<ResponseMetadata>"#,
                r#"<RequestId>7a62c49f-347e-4fc4-9331-6e8eEXAMPLE</RequestId>"#,
                r#"</ResponseMetadata></CreateAccessKeyResponse>"#
            )
        );
    }
}
