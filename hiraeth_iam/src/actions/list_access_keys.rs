use std::collections::HashMap;

use async_trait::async_trait;
use chrono::SecondsFormat;
use hiraeth_core::{
    AwsActionPayloadParseError, AwsActionResponseFormat, ResolvedRequest, TypedAwsAction,
    arn_util::user_arn,
    auth::AuthorizationCheck,
    tracing::{TraceContext, TraceRecorder},
};
use hiraeth_store::{IamStore, iam::AccessKey};
use serde::{Deserialize, Serialize};

use crate::{
    actions::util::{
        IAM_XMLNS, ResponseMetadata, parse_payload_error, requested_or_signing_user,
        response_metadata, validate_user_name,
    },
    error::IamError,
};

pub(crate) struct ListAccessKeysAction;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct ListAccessKeysRequest {
    user_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct ListAccessKeysResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    result: ListAccessKeysResult,
    response_metadata: ResponseMetadata,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
struct ListAccessKeysResult {
    access_key_metadata: AccessKeyMetadataXml,
    is_truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
struct AccessKeyMetadataXml {
    member: Vec<AccessKeyMetadataMemberXml>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
struct AccessKeyMetadataMemberXml {
    user_name: String,
    access_key_id: String,
    status: &'static str,
    create_date: String,
}

#[async_trait]
impl<S> TypedAwsAction<S> for ListAccessKeysAction
where
    S: IamStore + Send + Sync,
{
    type Request = ListAccessKeysRequest;
    type Response = ListAccessKeysResponse;
    type Error = IamError;

    fn name(&self) -> &'static str {
        "ListAccessKeys"
    }

    fn parse_error(&self, error: AwsActionPayloadParseError) -> IamError {
        parse_payload_error(error)
    }

    fn response_format(&self) -> AwsActionResponseFormat {
        AwsActionResponseFormat::Xml
    }

    async fn validate(
        &self,
        _request: &ResolvedRequest,
        list_request: &ListAccessKeysRequest,
        _store: &S,
    ) -> Result<(), IamError> {
        if let Some(user_name) = &list_request.user_name {
            validate_user_name(user_name)?;
        }

        Ok(())
    }

    async fn handle(
        &self,
        request: ResolvedRequest,
        list_request: ListAccessKeysRequest,
        store: &S,
        trace_context: &TraceContext,
        trace_recorder: &dyn TraceRecorder,
    ) -> Result<ListAccessKeysResponse, IamError> {
        let requested_user_name = list_request.user_name.clone();
        let target_user =
            requested_or_signing_user(&request, store, list_request.user_name.as_deref()).await?;
        let attributes = HashMap::from([
            (
                "requested_user_name".to_string(),
                requested_user_name.unwrap_or_else(|| "signing_user".to_string()),
            ),
            ("target_user_name".to_string(), target_user.name.clone()),
            ("target_user_id".to_string(), target_user.id.to_string()),
            ("account_id".to_string(), target_user.account_id.clone()),
        ]);

        let access_keys = trace_context
            .record_result_span(
                trace_recorder,
                "iam.access_key.list",
                "iam",
                attributes,
                async { store.list_access_keys_for_principal(target_user.id).await },
            )
            .await?;

        Ok(list_access_keys_response(
            &target_user.name,
            access_keys,
            request.request_id,
        ))
    }

    async fn resolve_authorization(
        &self,
        request: &ResolvedRequest,
        list_request: ListAccessKeysRequest,
        store: &S,
    ) -> Result<AuthorizationCheck, IamError> {
        let target_user =
            requested_or_signing_user(request, store, list_request.user_name.as_deref()).await?;

        Ok(AuthorizationCheck {
            action: "iam:ListAccessKeys".to_string(),
            resource: user_arn(
                &target_user.account_id,
                &target_user.path,
                &target_user.name,
            ),
            resource_policy: None,
        })
    }
}

fn list_access_keys_response(
    user_name: &str,
    access_keys: Vec<AccessKey>,
    request_id: impl Into<String>,
) -> ListAccessKeysResponse {
    ListAccessKeysResponse {
        xmlns: IAM_XMLNS,
        result: ListAccessKeysResult {
            access_key_metadata: AccessKeyMetadataXml {
                member: access_keys
                    .into_iter()
                    .map(|access_key| access_key_metadata_member(user_name, access_key))
                    .collect(),
            },
            is_truncated: false,
        },
        response_metadata: response_metadata(request_id),
    }
}

fn access_key_metadata_member(
    user_name: &str,
    access_key: AccessKey,
) -> AccessKeyMetadataMemberXml {
    AccessKeyMetadataMemberXml {
        user_name: user_name.to_string(),
        access_key_id: access_key.key_id,
        status: "Active",
        create_date: access_key
            .created_at
            .and_utc()
            .to_rfc3339_opts(SecondsFormat::Secs, true),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_core::{
        AuthContext, AwsAction, ResolvedRequest, TypedAwsActionAdapter,
        tracing::{NoopTraceRecorder, TraceContext},
        xml_body,
    };
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::iam::{AccessKey, InMemoryIamStore, Principal};

    use super::{ListAccessKeysAction, list_access_keys_response};

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

    fn access_key(key_id: &str, principal_id: i64) -> AccessKey {
        AccessKey {
            key_id: key_id.to_string(),
            principal_id,
            secret_key: "secret".to_string(),
            created_at: Utc
                .with_ymd_and_hms(2026, 4, 22, 12, 0, 0)
                .unwrap()
                .naive_utc(),
        }
    }

    fn store() -> InMemoryIamStore {
        InMemoryIamStore::new(
            [
                access_key("AKIA1111111111111111", 1),
                access_key("AKIA2222222222222222", 2),
                access_key("AKIA3333333333333333", 2),
            ],
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
                access_key: "AKIA1111111111111111".to_string(),
                principal: principal(1, "signing-user", "/"),
            },
            date: Utc.with_ymd_and_hms(2026, 4, 22, 12, 0, 0).unwrap(),
        }
    }

    #[tokio::test]
    async fn handle_lists_requested_users_access_keys() {
        let action = TypedAwsActionAdapter::new(ListAccessKeysAction);
        let response = action
            .handle(
                resolved_request(b"Action=ListAccessKeys&Version=2010-05-08&UserName=alice"),
                &store(),
                &TraceContext::new("test-request-id"),
                &NoopTraceRecorder,
            )
            .await;

        let body = String::from_utf8(response.body).expect("response should be utf8");

        assert_eq!(response.status_code, 200);
        assert!(body.contains("<ListAccessKeysResponse"));
        assert!(body.contains("<UserName>alice</UserName>"));
        assert!(body.contains("<AccessKeyId>AKIA2222222222222222</AccessKeyId>"));
        assert!(body.contains("<AccessKeyId>AKIA3333333333333333</AccessKeyId>"));
        assert!(!body.contains("<AccessKeyId>AKIA1111111111111111</AccessKeyId>"));
        assert!(body.contains("<IsTruncated>false</IsTruncated>"));
    }

    #[tokio::test]
    async fn handle_uses_signing_user_when_user_name_is_omitted() {
        let action = TypedAwsActionAdapter::new(ListAccessKeysAction);
        let response = action
            .handle(
                resolved_request(b"Action=ListAccessKeys&Version=2010-05-08"),
                &store(),
                &TraceContext::new("test-request-id"),
                &NoopTraceRecorder,
            )
            .await;

        let body = String::from_utf8(response.body).expect("response should be utf8");

        assert_eq!(response.status_code, 200);
        assert!(body.contains("<UserName>signing-user</UserName>"));
        assert!(body.contains("<AccessKeyId>AKIA1111111111111111</AccessKeyId>"));
        assert!(!body.contains("<AccessKeyId>AKIA2222222222222222</AccessKeyId>"));
    }

    #[tokio::test]
    async fn resolve_authorization_uses_target_user_arn() {
        let action = TypedAwsActionAdapter::new(ListAccessKeysAction);
        let check = action
            .resolve_authorization(
                &resolved_request(b"Action=ListAccessKeys&Version=2010-05-08&UserName=alice"),
                &store(),
            )
            .await
            .expect("authorization check should resolve");

        assert_eq!(check.action, "iam:ListAccessKeys");
        assert_eq!(
            check.resource,
            "arn:aws:iam::123456789012:user/engineering/alice"
        );
    }

    #[test]
    fn list_access_keys_response_serializes_expected_xml_shape() {
        let xml = String::from_utf8(
            xml_body(&list_access_keys_response(
                "alice",
                vec![access_key("AKIA2222222222222222", 2)],
                "request-id",
            ))
            .expect("xml should serialize"),
        )
        .expect("xml should be utf8");

        assert!(xml.contains(
            r#"<ListAccessKeysResponse xmlns="https://iam.amazonaws.com/doc/2010-05-08/">"#
        ));
        assert!(xml.contains("<AccessKeyMetadata><member>"));
        assert!(xml.contains("<UserName>alice</UserName>"));
        assert!(xml.contains("<ResponseMetadata><RequestId>request-id</RequestId>"));
    }
}
