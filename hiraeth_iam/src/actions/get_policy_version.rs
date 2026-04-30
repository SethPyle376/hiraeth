use std::collections::HashMap;

use async_trait::async_trait;
use chrono::SecondsFormat;
use hiraeth_core::{
    AwsActionPayloadParseError, AwsActionResponseFormat, ResolvedRequest, TypedAwsAction,
    auth::AuthorizationCheck,
    tracing::{TraceContext, TraceRecorder},
};
use hiraeth_store::{IamStore, iam::ManagedPolicy};
use serde::{Deserialize, Serialize};

use crate::{
    actions::util::{
        DEFAULT_POLICY_VERSION_ID, IAM_XMLNS, ResponseMetadata, parse_payload_error,
        parse_policy_arn,
    },
    error::IamError,
};

pub(crate) struct GetPolicyVersionAction;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct GetPolicyVersionRequest {
    policy_arn: String,
    #[serde(default)]
    version_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename = "GetPolicyVersionResponse")]
pub(crate) struct GetPolicyVersionResponse {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    #[serde(rename = "GetPolicyVersionResult")]
    result: GetPolicyVersionResult,
    #[serde(rename = "ResponseMetadata")]
    response_metadata: ResponseMetadata,
}

#[derive(Debug, Clone, Serialize)]
struct GetPolicyVersionResult {
    #[serde(rename = "PolicyVersion")]
    policy_version: PolicyVersionXml,
}

#[derive(Debug, Clone, Serialize)]
struct PolicyVersionXml {
    #[serde(rename = "Document")]
    document: String,
    #[serde(rename = "VersionId")]
    version_id: String,
    #[serde(rename = "IsDefaultVersion")]
    is_default_version: bool,
    #[serde(rename = "CreateDate")]
    create_date: String,
}

#[async_trait]
impl<S> TypedAwsAction<S> for GetPolicyVersionAction
where
    S: IamStore + Send + Sync,
{
    type Request = GetPolicyVersionRequest;
    type Response = GetPolicyVersionResponse;
    type Error = IamError;

    fn name(&self) -> &'static str {
        "GetPolicyVersion"
    }

    fn parse_error(&self, error: AwsActionPayloadParseError) -> IamError {
        parse_payload_error(error)
    }

    fn response_format(&self) -> AwsActionResponseFormat {
        AwsActionResponseFormat::Xml
    }

    async fn handle(
        &self,
        request: ResolvedRequest,
        get_request: GetPolicyVersionRequest,
        store: &S,
        trace_context: &TraceContext,
        trace_recorder: &dyn TraceRecorder,
    ) -> Result<GetPolicyVersionResponse, IamError> {
        let policy_arn = parse_policy_arn(&get_request.policy_arn)?;
        let version_id = normalize_version_id(&get_request.version_id);
        let attributes = HashMap::from([
            ("account_id".to_string(), policy_arn.account_id.clone()),
            ("policy_arn".to_string(), get_request.policy_arn.clone()),
            ("policy_name".to_string(), policy_arn.policy_name.clone()),
            ("policy_path".to_string(), policy_arn.policy_path.clone()),
            ("version_id".to_string(), version_id.clone()),
        ]);

        let policy = trace_context
            .record_result_span(
                trace_recorder,
                "iam.policy_version.get",
                "iam",
                attributes,
                async {
                    store
                        .get_managed_policy(
                            &policy_arn.account_id,
                            &policy_arn.policy_name,
                            &policy_arn.policy_path,
                        )
                        .await?
                        .ok_or_else(|| {
                            IamError::NoSuchEntity(format!(
                                "Policy {} does not exist",
                                get_request.policy_arn
                            ))
                        })
                },
            )
            .await?;

        Ok(get_policy_version_response(
            &policy,
            &version_id,
            request.request_id,
        ))
    }

    async fn resolve_authorization(
        &self,
        _request: &ResolvedRequest,
        get_request: GetPolicyVersionRequest,
        _store: &S,
    ) -> Result<AuthorizationCheck, IamError> {
        Ok(AuthorizationCheck {
            action: "iam:GetPolicyVersion".to_string(),
            resource: get_request.policy_arn,
            resource_policy: None,
        })
    }
}

fn get_policy_version_response(
    policy: &ManagedPolicy,
    version_id: &str,
    request_id: impl Into<String>,
) -> GetPolicyVersionResponse {
    GetPolicyVersionResponse {
        xmlns: IAM_XMLNS,
        result: GetPolicyVersionResult {
            policy_version: PolicyVersionXml {
                document: url_encode_policy_document(&policy.policy_document),
                version_id: version_id.to_string(),
                is_default_version: true,
                create_date: policy
                    .updated_at
                    .and_utc()
                    .to_rfc3339_opts(SecondsFormat::Secs, true),
            },
        },
        response_metadata: ResponseMetadata {
            request_id: request_id.into(),
        },
    }
}

fn normalize_version_id(version_id: &str) -> String {
    let version_id = version_id.trim();
    if version_id.is_empty() {
        DEFAULT_POLICY_VERSION_ID.to_string()
    } else {
        version_id.to_string()
    }
}

fn url_encode_policy_document(document: &str) -> String {
    document
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![byte as char]
            }
            _ => format!("%{byte:02X}").chars().collect(),
        })
        .collect()
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
    use hiraeth_store::iam::{AccessKey, InMemoryIamStore, ManagedPolicy, Principal};

    use super::{
        GetPolicyVersionAction, get_policy_version_response, normalize_version_id,
        url_encode_policy_document,
    };

    fn principal(id: i64, name: &str) -> Principal {
        Principal {
            id,
            account_id: "123456789012".to_string(),
            kind: "user".to_string(),
            name: name.to_string(),
            path: "/".to_string(),
            user_id: format!("AIDATESTUSER{id:08}"),
            created_at: Utc
                .with_ymd_and_hms(2026, 4, 28, 12, 0, 0)
                .unwrap()
                .naive_utc(),
        }
    }

    fn policy() -> ManagedPolicy {
        ManagedPolicy {
            id: 10,
            policy_id: "AIDAPOLICY00000001".to_string(),
            account_id: "123456789012".to_string(),
            policy_name: "orders-readonly".to_string(),
            policy_path: Some("/dev/team-a/".to_string()),
            policy_document: r#"{"Version":"2012-10-17","Statement":[]}"#.to_string(),
            created_at: Utc
                .with_ymd_and_hms(2026, 4, 28, 12, 0, 0)
                .unwrap()
                .naive_utc(),
            updated_at: Utc
                .with_ymd_and_hms(2026, 4, 29, 12, 0, 0)
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
            [principal(1, "signing-user")],
            [],
            [policy()],
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
                principal: principal(1, "signing-user"),
            },
            date: Utc.with_ymd_and_hms(2026, 4, 29, 12, 0, 0).unwrap(),
        }
    }

    #[tokio::test]
    async fn handle_returns_policy_document_for_requested_version() {
        let action = TypedAwsActionAdapter::new(GetPolicyVersionAction);
        let response = action
            .handle(
                resolved_request(
                    b"Action=GetPolicyVersion&Version=2010-05-08&PolicyArn=arn%3Aaws%3Aiam%3A%3A123456789012%3Apolicy%2Fdev%2Fteam-a%2Forders-readonly&VersionId=v12",
                ),
                &store(),
                &TraceContext::new("test-request-id"),
                &NoopTraceRecorder,
            )
            .await;

        let body = String::from_utf8(response.body).expect("response should be utf8");

        assert_eq!(response.status_code, 200);
        assert!(body.contains("<GetPolicyVersionResponse"));
        assert!(body.contains("<VersionId>v12</VersionId>"));
        assert!(body.contains("<IsDefaultVersion>true</IsDefaultVersion>"));
        assert!(body.contains("<Document>%7B%22Version%22%3A%222012-10-17%22"));
    }

    #[tokio::test]
    async fn handle_defaults_empty_version_id_to_v1() {
        let action = TypedAwsActionAdapter::new(GetPolicyVersionAction);
        let response = action
            .handle(
                resolved_request(
                    b"Action=GetPolicyVersion&Version=2010-05-08&PolicyArn=arn%3Aaws%3Aiam%3A%3A123456789012%3Apolicy%2Fdev%2Fteam-a%2Forders-readonly&VersionId=",
                ),
                &store(),
                &TraceContext::new("test-request-id"),
                &NoopTraceRecorder,
            )
            .await;

        let body = String::from_utf8(response.body).expect("response should be utf8");

        assert_eq!(response.status_code, 200);
        assert!(body.contains("<VersionId>v1</VersionId>"));
    }

    #[tokio::test]
    async fn resolve_authorization_uses_policy_arn() {
        let action = TypedAwsActionAdapter::new(GetPolicyVersionAction);
        let check = action
            .resolve_authorization(
                &resolved_request(
                    b"Action=GetPolicyVersion&Version=2010-05-08&PolicyArn=arn%3Aaws%3Aiam%3A%3A123456789012%3Apolicy%2Fdev%2Fteam-a%2Forders-readonly&VersionId=v1",
                ),
                &store(),
            )
            .await
            .expect("authorization check should resolve");

        assert_eq!(check.action, "iam:GetPolicyVersion");
        assert_eq!(
            check.resource,
            "arn:aws:iam::123456789012:policy/dev/team-a/orders-readonly"
        );
    }

    #[test]
    fn policy_document_url_encoding_uses_percent_encoding() {
        assert_eq!(
            url_encode_policy_document(r#"{"a b":"*"}"#),
            "%7B%22a%20b%22%3A%22%2A%22%7D"
        );
    }

    #[test]
    fn normalize_version_id_defaults_empty_values() {
        assert_eq!(normalize_version_id(""), "v1");
        assert_eq!(normalize_version_id("  "), "v1");
        assert_eq!(normalize_version_id("v7"), "v7");
    }

    #[test]
    fn get_policy_version_response_serializes_expected_xml_shape() {
        let xml = String::from_utf8(
            xml_body(&get_policy_version_response(&policy(), "v1", "request-id"))
                .expect("xml should serialize"),
        )
        .expect("xml should be utf8");

        assert!(xml.contains(
            r#"<GetPolicyVersionResponse xmlns="https://iam.amazonaws.com/doc/2010-05-08/">"#
        ));
        assert!(xml.contains("<GetPolicyVersionResult><PolicyVersion>"));
        assert!(xml.contains("<VersionId>v1</VersionId>"));
        assert!(xml.contains("<ResponseMetadata><RequestId>request-id</RequestId>"));
    }
}
