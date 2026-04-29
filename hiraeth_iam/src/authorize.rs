use std::collections::HashMap;

use async_trait::async_trait;
use hiraeth_core::{
    AuthContext, ResolvedRequest, ServiceResponse,
    auth::{
        AuthorizationCheck, Policy, PolicyEvalResult, PolicyPrincipal, evaluate_identity_policy,
        evaluate_resource_policy,
    },
    tracing::{TraceContext, TraceRecorder, TraceSpanTimer},
};
use hiraeth_router::{AuthorizationResult, Authorizer};
use hiraeth_store::{IamStore, iam::PrincipalInlinePolicyStore};

use crate::{AuthorizationMode, IamService};

#[async_trait]
impl<S> Authorizer for IamService<S>
where
    S: IamStore + Send + Sync,
{
    async fn authorize(
        &self,
        request: &ResolvedRequest,
        check: &AuthorizationCheck,
        trace_context: &TraceContext,
        trace_recorder: &dyn TraceRecorder,
    ) -> AuthorizationResult {
        let authz_timer = trace_context.start_span();
        let resource_principal = policy_principal_from_request(request);
        let identity_policy_result = evaluate_principal_inline_policies(
            &self.store,
            request.auth_context.principal.id,
            &check.resource,
            &check.action,
        )
        .await
        .unwrap_or_else(|error| {
            tracing::error!(
                principal_id = request.auth_context.principal.id,
                resource = %check.resource,
                action = %check.action,
                "failed to evaluate inline policies: {error}"
            );
            PolicyEvalResult::Denied
        });

        let managed_policy_result = evaluate_managed_policies(
            &self.store,
            request.auth_context.principal.id,
            &check.resource,
            &check.action,
        )
        .await
        .unwrap_or_else(|error| {
            tracing::error!(
                principal_id = request.auth_context.principal.id,
                resource = %check.resource,
                action = %check.action,
                "failed to evaluate managed policies: {error}"
            );
            PolicyEvalResult::Denied
        });

        let resource_policy_result = match (&resource_principal, check.resource_policy.as_ref()) {
            (Some(principal), Some(policy)) => {
                evaluate_resource_policy(principal, &check.resource, &check.action, policy)
            }
            _ => PolicyEvalResult::NotApplicable,
        };
        let policy_result = combine_policy_results([
            identity_policy_result,
            managed_policy_result,
            resource_policy_result,
        ]);

        let authz_result = match policy_result {
            PolicyEvalResult::Allowed => AuthorizationResult::Allow,
            PolicyEvalResult::Denied | PolicyEvalResult::NotApplicable => AuthorizationResult::Deny,
        };

        let effective_result = match self.mode {
            AuthorizationMode::Enforce => authz_result,
            AuthorizationMode::Audit => {
                tracing::info!(
                    "Audit authz: principal={:?}, resource={}, action={}, result={:?}",
                    resource_principal.as_ref(),
                    check.resource,
                    check.action,
                    authz_result
                );
                AuthorizationResult::Allow // allow the request but log the result
            }
            AuthorizationMode::Off => AuthorizationResult::Allow, // allow all requests
        };

        record_authz_span(
            trace_context,
            trace_recorder,
            authz_timer,
            &self.mode,
            authz_result,
            effective_result,
            check,
        )
        .await;

        effective_result
    }

    fn unauthorized_response(&self) -> ServiceResponse {
        ServiceResponse {
            status_code: 403,
            body: vec![],
            headers: vec![],
        }
    }
}

async fn record_authz_span(
    trace_context: &TraceContext,
    trace_recorder: &dyn TraceRecorder,
    timer: TraceSpanTimer,
    mode: &AuthorizationMode,
    evaluated_result: AuthorizationResult,
    effective_result: AuthorizationResult,
    check: &AuthorizationCheck,
) {
    if let Err(error) = trace_context
        .record_span(
            trace_recorder,
            timer,
            "authz.evaluate",
            "iam",
            evaluated_result.as_trace_status(),
            HashMap::from([
                ("action".to_string(), check.action.clone()),
                ("resource".to_string(), check.resource.clone()),
                (
                    "mode".to_string(),
                    authorization_mode_name(mode).to_string(),
                ),
                (
                    "effective_result".to_string(),
                    effective_result.as_trace_status().to_string(),
                ),
            ]),
        )
        .await
    {
        tracing::warn!(error = ?error, span = "authz.evaluate", "failed to record trace span");
    }
}

fn authorization_mode_name(mode: &AuthorizationMode) -> &'static str {
    match mode {
        AuthorizationMode::Enforce => "enforce",
        AuthorizationMode::Audit => "audit",
        AuthorizationMode::Off => "off",
    }
}

fn combine_policy_results(results: impl IntoIterator<Item = PolicyEvalResult>) -> PolicyEvalResult {
    results
        .into_iter()
        .fold(PolicyEvalResult::NotApplicable, |acc, result| {
            match (acc, result) {
                (PolicyEvalResult::Denied, _) => PolicyEvalResult::Denied,
                (_, PolicyEvalResult::Denied) => PolicyEvalResult::Denied,
                (PolicyEvalResult::Allowed, _) => PolicyEvalResult::Allowed,
                (_, PolicyEvalResult::Allowed) => PolicyEvalResult::Allowed,
                _ => PolicyEvalResult::NotApplicable,
            }
        })
}

async fn evaluate_principal_inline_policies<S: IamStore + Send + Sync>(
    store: &S,
    principal_id: i64,
    resource: &str,
    action: &str,
) -> Result<PolicyEvalResult, String> {
    let policies = store
        .get_inline_policies_for_principal(principal_id)
        .await
        .map_err(|error| error.to_string())?;

    let policy_results = policies
        .into_iter()
        .map(|policy| {
            serde_json::from_str::<Policy>(&policy.policy_document)
                .map(|policy_document| evaluate_identity_policy(resource, action, &policy_document))
                .map_err(|error| {
                    format!(
                        "invalid policy document for inline policy {}: {}",
                        policy.policy_name, error
                    )
                })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(combine_policy_results(policy_results))
}

async fn evaluate_managed_policies<S: IamStore + Send + Sync>(
    store: &S,
    principal_id: i64,
    resource: &str,
    action: &str,
) -> Result<PolicyEvalResult, String> {
    let policies = store
        .get_managed_policies_attached_to_principal(principal_id)
        .await
        .map_err(|error| error.to_string())?;

    let policy_results = policies
        .into_iter()
        .map(|policy| {
            serde_json::from_str::<Policy>(&policy.policy_document)
                .map(|policy_document| evaluate_identity_policy(resource, action, &policy_document))
                .map_err(|error| {
                    format!(
                        "invalid policy document for managed policy {}: {}",
                        policy.policy_name, error
                    )
                })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(combine_policy_results(policy_results))
}

fn policy_principal_from_request(request: &ResolvedRequest) -> Option<PolicyPrincipal> {
    let request_principal = request.auth_context.principal.clone();
    match request_principal.kind.as_str() {
        "user" => Some(PolicyPrincipal::User {
            account_id: request_principal.account_id.clone(),
            user_name: request_principal.name.clone(),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Mutex};

    use async_trait::async_trait;
    use chrono::{TimeZone, Utc};
    use hiraeth_core::{
        AuthContext, ResolvedRequest,
        auth::{AuthorizationCheck, Policy, PolicyPrincipal},
        tracing::{
            CompletedRequestTrace, NoopTraceRecorder, TraceContext, TraceRecordError,
            TraceRecorder, TraceSpanRecord,
        },
    };
    use hiraeth_http::IncomingRequest;
    use hiraeth_router::{AuthorizationResult, Authorizer};
    use hiraeth_store::{
        StoreError,
        iam::{
            AccessKey, AccessKeyStore, ManagedPolicy, ManagedPolicyStore, NewManagedPolicy,
            NewPrincipal, Principal, PrincipalInlinePolicy, PrincipalInlinePolicyStore,
            PrincipalStore,
        },
    };

    use crate::{AuthorizationMode, IamService};

    use super::policy_principal_from_request;

    #[derive(Clone, Default)]
    struct TestIamStore {
        inline_policies: Vec<PrincipalInlinePolicy>,
        managed_policies: Vec<ManagedPolicy>,
    }

    #[derive(Default)]
    struct RecordingTraceRecorder {
        spans: Mutex<Vec<TraceSpanRecord>>,
    }

    #[async_trait]
    impl TraceRecorder for RecordingTraceRecorder {
        async fn record_request_trace(
            &self,
            _trace: CompletedRequestTrace,
        ) -> Result<(), TraceRecordError> {
            unreachable!("authorization tests only record spans")
        }

        async fn record_span(&self, span: TraceSpanRecord) -> Result<(), TraceRecordError> {
            self.spans
                .lock()
                .expect("trace recorder mutex should not be poisoned")
                .push(span);
            Ok(())
        }
    }

    #[async_trait]
    impl AccessKeyStore for TestIamStore {
        async fn get_secret_key(
            &self,
            _access_key: &str,
        ) -> Result<Option<AccessKey>, hiraeth_store::StoreError> {
            Ok(None)
        }

        async fn list_access_keys_for_principal(
            &self,
            _principal_id: i64,
        ) -> Result<Vec<AccessKey>, hiraeth_store::StoreError> {
            Ok(vec![])
        }

        async fn insert_secret_key(
            &self,
            _access_key: &str,
            _secret_key: &str,
            _principal_id: i64,
        ) -> Result<AccessKey, hiraeth_store::StoreError> {
            unreachable!("authorization tests do not insert access keys")
        }

        async fn delete_access_key_for_principal(
            &self,
            _principal_id: i64,
            _access_key: &str,
        ) -> Result<(), hiraeth_store::StoreError> {
            unreachable!("authorization tests do not delete access keys")
        }
    }

    #[async_trait]
    impl PrincipalStore for TestIamStore {
        async fn get_principal(
            &self,
            _principal_id: i64,
        ) -> Result<Option<Principal>, hiraeth_store::StoreError> {
            Ok(None)
        }

        async fn get_principal_by_identity(
            &self,
            _account_id: &str,
            _kind: &str,
            _name: &str,
        ) -> Result<Option<Principal>, hiraeth_store::StoreError> {
            Ok(None)
        }

        async fn list_principals(&self) -> Result<Vec<Principal>, hiraeth_store::StoreError> {
            Ok(vec![])
        }

        async fn create_principal(
            &self,
            principal: NewPrincipal,
        ) -> Result<Principal, hiraeth_store::StoreError> {
            Ok(Principal {
                id: 999,
                account_id: principal.account_id,
                kind: principal.kind,
                name: principal.name,
                path: principal.path,
                user_id: principal.user_id,
                created_at: Utc::now().naive_utc(),
            })
        }

        async fn delete_principal(
            &self,
            _principal_id: i64,
        ) -> Result<(), hiraeth_store::StoreError> {
            unreachable!("authorization tests do not delete principals")
        }

        async fn delete_user(
            &self,
            _account_id: &str,
            _name: &str,
        ) -> Result<(), hiraeth_store::StoreError> {
            unreachable!("authorization tests do not delete users")
        }
    }

    #[async_trait]
    impl PrincipalInlinePolicyStore for TestIamStore {
        async fn get_inline_policies_for_principal(
            &self,
            principal_id: i64,
        ) -> Result<Vec<PrincipalInlinePolicy>, hiraeth_store::StoreError> {
            Ok(self
                .inline_policies
                .iter()
                .filter(|policy| policy.principal_id == principal_id)
                .cloned()
                .collect())
        }

        async fn put_inline_policy(
            &self,
            _principal_id: i64,
            _policy_name: &str,
            _policy_document: &str,
        ) -> Result<PrincipalInlinePolicy, hiraeth_store::StoreError> {
            unreachable!("authorization tests do not put inline policies")
        }

        async fn delete_inline_policy(
            &self,
            _principal_id: i64,
            _policy_name: &str,
        ) -> Result<(), hiraeth_store::StoreError> {
            unreachable!("authorization tests do not delete inline policies")
        }
    }

    #[async_trait]
    impl ManagedPolicyStore for TestIamStore {
        async fn insert_managed_policy(
            &self,
            policy: NewManagedPolicy,
        ) -> Result<ManagedPolicy, StoreError> {
            todo!()
        }

        async fn get_managed_policy(
            &self,
            _account_id: &str,
            _policy_name: &str,
            _policy_path: &str,
        ) -> Result<Option<ManagedPolicy>, StoreError> {
            Ok(None)
        }

        async fn list_managed_policies(&self) -> Result<Vec<ManagedPolicy>, StoreError> {
            unreachable!("authorization tests do not list managed policies")
        }

        async fn update_managed_policy_document(
            &self,
            _policy_id: i64,
            _policy_document: &str,
        ) -> Result<ManagedPolicy, StoreError> {
            unreachable!("authorization tests do not update managed policies")
        }

        async fn attach_policy_to_principal(
            &self,
            _policy_id: i64,
            _principal_id: i64,
        ) -> Result<(), StoreError> {
            unreachable!("authorization tests do not attach managed policies")
        }

        async fn detach_policy_from_principal(
            &self,
            _policy_id: i64,
            _principal_id: i64,
        ) -> Result<(), StoreError> {
            unreachable!("authorization tests do not detach managed policies")
        }

        async fn delete_managed_policy(
            &self,
            _account_id: &str,
            _policy_name: &str,
            _policy_path: &str,
        ) -> Result<(), StoreError> {
            unreachable!("authorization tests do not delete managed policies")
        }

        async fn get_managed_policies_attached_to_principal(
            &self,
            principal_id: i64,
        ) -> Result<Vec<ManagedPolicy>, StoreError> {
            Ok(self
                .managed_policies
                .iter()
                .filter(|policy| policy.id == principal_id)
                .cloned()
                .collect())
        }
    }

    fn service(
        mode: AuthorizationMode,
        inline_policies: impl IntoIterator<Item = PrincipalInlinePolicy>,
    ) -> IamService<TestIamStore> {
        IamService::new(
            mode,
            TestIamStore {
                inline_policies: inline_policies.into_iter().collect(),
                managed_policies: vec![],
            },
        )
    }

    fn service_with_managed(
        mode: AuthorizationMode,
        inline_policies: impl IntoIterator<Item = PrincipalInlinePolicy>,
        managed_policies: impl IntoIterator<Item = ManagedPolicy>,
    ) -> IamService<TestIamStore> {
        IamService::new(
            mode,
            TestIamStore {
                inline_policies: inline_policies.into_iter().collect(),
                managed_policies: managed_policies.into_iter().collect(),
            },
        )
    }

    fn resolved_request(kind: &str) -> ResolvedRequest {
        ResolvedRequest {
            request_id: "test-request-id".to_string(),
            request: IncomingRequest {
                host: "sqs.us-east-1.amazonaws.com".to_string(),
                method: "POST".to_string(),
                path: "/".to_string(),
                query: None,
                headers: HashMap::new(),
                body: Vec::new(),
            },
            service: "sqs".to_string(),
            region: "us-east-1".to_string(),
            auth_context: AuthContext {
                access_key: "AKIAIOSFODNN7EXAMPLE".to_string(),
                principal: Principal {
                    id: 1,
                    account_id: "123456789012".to_string(),
                    kind: kind.to_string(),
                    name: "alice".to_string(),
                    path: "/".to_string(),
                    user_id: "AIDATESTUSER000001".to_string(),
                    created_at: Utc
                        .with_ymd_and_hms(2026, 4, 20, 12, 0, 0)
                        .unwrap()
                        .naive_utc(),
                },
            },
            date: Utc.with_ymd_and_hms(2026, 4, 20, 12, 0, 0).unwrap(),
        }
    }

    fn user_principal() -> PolicyPrincipal {
        PolicyPrincipal::User {
            account_id: "123456789012".to_string(),
            user_name: "alice".to_string(),
        }
    }

    fn policy(effect: &str, action: &str, resource: &str) -> Policy {
        serde_json::from_str(&format!(
            r#"{{
                "Version":"2012-10-17",
                "Statement":[
                    {{
                        "Effect":"{effect}",
                        "Principal":{{"AWS":"arn:aws:iam::123456789012:user/alice"}},
                        "Action":"{action}",
                        "Resource":"{resource}"
                    }}
                ]
            }}"#
        ))
        .expect("test policy should deserialize")
    }

    fn check(resource_policy: Option<Policy>) -> AuthorizationCheck {
        AuthorizationCheck {
            action: "sqs:SendMessage".to_string(),
            resource: "arn:aws:sqs:us-east-1:123456789012:orders".to_string(),
            resource_policy,
        }
    }

    async fn authorize(
        authorizer: &impl Authorizer,
        request: ResolvedRequest,
        check: AuthorizationCheck,
    ) -> AuthorizationResult {
        let trace_context = TraceContext::new(request.request_id.clone());
        authorizer
            .authorize(&request, &check, &trace_context, &NoopTraceRecorder)
            .await
    }

    fn auth_check(
        action: &str,
        resource: &str,
        resource_policy: Option<Policy>,
    ) -> AuthorizationCheck {
        AuthorizationCheck {
            action: action.to_string(),
            resource: resource.to_string(),
            resource_policy,
        }
    }

    fn inline_policy(
        effect: &str,
        action: &str,
        resource: &str,
        principal_id: i64,
    ) -> PrincipalInlinePolicy {
        PrincipalInlinePolicy {
            id: 1,
            principal_id,
            policy_name: format!("{}-{}", effect.to_lowercase(), action),
            policy_document: format!(
                r#"{{
                    "Version":"2012-10-17",
                    "Statement":[
                        {{
                            "Effect":"{effect}",
                            "Action":"{action}",
                            "Resource":"{resource}"
                        }}
                    ]
                }}"#
            ),
            created_at: Utc
                .with_ymd_and_hms(2026, 4, 20, 12, 0, 0)
                .unwrap()
                .naive_utc(),
            updated_at: Utc
                .with_ymd_and_hms(2026, 4, 20, 12, 0, 0)
                .unwrap()
                .naive_utc(),
        }
    }

    fn managed_policy(
        effect: &str,
        action: &str,
        resource: &str,
        principal_id: i64,
    ) -> ManagedPolicy {
        ManagedPolicy {
            id: principal_id,
            policy_id: format!("AIDAPOLICY{:08}", principal_id),
            account_id: "123456789012".to_string(),
            policy_name: format!("managed-{}-{}", effect.to_lowercase(), action),
            policy_path: Some("/".to_string()),
            policy_document: format!(
                r#"{{
                    "Version":"2012-10-17",
                    "Statement":[
                        {{
                            "Effect":"{effect}",
                            "Action":"{action}",
                            "Resource":"{resource}"
                        }}
                    ]
                }}"#
            ),
            created_at: Utc
                .with_ymd_and_hms(2026, 4, 20, 12, 0, 0)
                .unwrap()
                .naive_utc(),
            updated_at: Utc
                .with_ymd_and_hms(2026, 4, 20, 12, 0, 0)
                .unwrap()
                .naive_utc(),
        }
    }

    #[tokio::test]
    async fn get_principal_maps_project_user_to_policy_user() {
        let principal = policy_principal_from_request(&resolved_request("user"));

        assert_eq!(principal, Some(user_principal()));
    }

    #[tokio::test]
    async fn get_principal_returns_none_for_unknown_project_principal_kind() {
        let principal = policy_principal_from_request(&resolved_request("unknown"));

        assert_eq!(principal, None);
    }

    #[tokio::test]
    async fn enforce_mode_allows_matching_resource_policy() {
        let result = authorize(
            &service(AuthorizationMode::Enforce, []),
            resolved_request("user"),
            check(Some(policy(
                "Allow",
                "sqs:SendMessage",
                "arn:aws:sqs:us-east-1:123456789012:orders",
            ))),
        )
        .await;

        assert_eq!(result, AuthorizationResult::Allow);
    }

    #[tokio::test]
    async fn enforce_mode_denies_matching_deny_policy() {
        let result = authorize(
            &service(AuthorizationMode::Enforce, []),
            resolved_request("user"),
            check(Some(policy(
                "Deny",
                "sqs:SendMessage",
                "arn:aws:sqs:us-east-1:123456789012:orders",
            ))),
        )
        .await;

        assert_eq!(result, AuthorizationResult::Deny);
    }

    #[tokio::test]
    async fn enforce_mode_denies_missing_or_not_applicable_resource_policy() {
        let authorizer = service(AuthorizationMode::Enforce, []);
        let no_policy_result = authorize(&authorizer, resolved_request("user"), check(None)).await;
        let not_applicable_result = authorize(
            &authorizer,
            resolved_request("user"),
            check(Some(policy(
                "Allow",
                "sqs:ReceiveMessage",
                "arn:aws:sqs:us-east-1:123456789012:orders",
            ))),
        )
        .await;

        assert_eq!(no_policy_result, AuthorizationResult::Deny);
        assert_eq!(not_applicable_result, AuthorizationResult::Deny);
    }

    #[tokio::test]
    async fn audit_mode_allows_even_when_policy_would_deny() {
        let result = authorize(
            &service(AuthorizationMode::Audit, []),
            resolved_request("user"),
            check(None),
        )
        .await;

        assert_eq!(result, AuthorizationResult::Allow);
    }

    #[tokio::test]
    async fn audit_mode_records_evaluated_result_separately_from_effective_result() {
        let authorizer = service(AuthorizationMode::Audit, []);
        let request = resolved_request("user");
        let check = check(None);
        let trace_context = TraceContext::new("trace-request-id");
        let trace_recorder = RecordingTraceRecorder::default();

        let result = authorizer
            .authorize(&request, &check, &trace_context, &trace_recorder)
            .await;

        assert_eq!(result, AuthorizationResult::Allow);

        let spans = trace_recorder
            .spans
            .lock()
            .expect("trace recorder mutex should not be poisoned");
        assert_eq!(spans.len(), 1);

        let span = &spans[0];
        assert_eq!(span.name, "authz.evaluate");
        assert_eq!(span.layer, "iam");
        assert_eq!(span.status, "deny");
        assert_eq!(
            span.attributes.get("mode").map(String::as_str),
            Some("audit")
        );
        assert_eq!(
            span.attributes.get("effective_result").map(String::as_str),
            Some("allow")
        );
        assert_eq!(
            span.attributes.get("action").map(String::as_str),
            Some("sqs:SendMessage")
        );
        assert_eq!(
            span.attributes.get("resource").map(String::as_str),
            Some("arn:aws:sqs:us-east-1:123456789012:orders")
        );
    }

    #[tokio::test]
    async fn off_mode_allows_without_evaluating_policy() {
        let result = authorize(
            &service(AuthorizationMode::Off, []),
            resolved_request("user"),
            check(None),
        )
        .await;

        assert_eq!(result, AuthorizationResult::Allow);
    }

    #[tokio::test]
    async fn authorization_denies_when_project_principal_cannot_map_to_policy_principal() {
        let result = authorize(
            &service(AuthorizationMode::Enforce, []),
            resolved_request("unknown"),
            check(Some(policy(
                "Allow",
                "sqs:SendMessage",
                "arn:aws:sqs:us-east-1:123456789012:orders",
            ))),
        )
        .await;

        assert_eq!(result, AuthorizationResult::Deny);
    }

    #[test]
    fn unauthorized_response_is_forbidden() {
        let response = service(AuthorizationMode::Enforce, []).unauthorized_response();

        assert_eq!(response.status_code, 403);
        assert!(response.body.is_empty());
    }

    #[tokio::test]
    async fn enforce_mode_allows_matching_inline_identity_policy_without_resource_policy() {
        let result = authorize(
            &service(
                AuthorizationMode::Enforce,
                [inline_policy(
                    "Allow",
                    "sqs:SendMessage",
                    "arn:aws:sqs:us-east-1:123456789012:orders",
                    1,
                )],
            ),
            resolved_request("user"),
            check(None),
        )
        .await;

        assert_eq!(result, AuthorizationResult::Allow);
    }

    #[tokio::test]
    async fn enforce_mode_allows_matching_managed_policy_without_resource_policy() {
        let result = authorize(
            &service_with_managed(
                AuthorizationMode::Enforce,
                [],
                [managed_policy(
                    "Allow",
                    "sqs:SendMessage",
                    "arn:aws:sqs:us-east-1:123456789012:orders",
                    1,
                )],
            ),
            resolved_request("user"),
            check(None),
        )
        .await;

        assert_eq!(result, AuthorizationResult::Allow);
    }

    #[tokio::test]
    async fn enforce_mode_denies_when_inline_identity_policy_denies_resource_allowed_by_resource_policy()
     {
        let result = authorize(
            &service(
                AuthorizationMode::Enforce,
                [inline_policy(
                    "Deny",
                    "sqs:SendMessage",
                    "arn:aws:sqs:us-east-1:123456789012:orders",
                    1,
                )],
            ),
            resolved_request("user"),
            check(Some(policy(
                "Allow",
                "sqs:SendMessage",
                "arn:aws:sqs:us-east-1:123456789012:orders",
            ))),
        )
        .await;

        assert_eq!(result, AuthorizationResult::Deny);
    }

    #[tokio::test]
    async fn enforce_mode_allows_inline_identity_policy_even_without_resource_principal_mapping() {
        let result = authorize(
            &service(
                AuthorizationMode::Enforce,
                [inline_policy(
                    "Allow",
                    "sqs:SendMessage",
                    "arn:aws:sqs:us-east-1:123456789012:orders",
                    1,
                )],
            ),
            resolved_request("unknown"),
            check(None),
        )
        .await;

        assert_eq!(result, AuthorizationResult::Allow);
    }

    #[tokio::test]
    async fn enforce_mode_allows_seeded_admin_style_inline_policy_for_send_message() {
        let result = authorize(
            &service(
                AuthorizationMode::Enforce,
                [inline_policy("Allow", "*", "arn:aws:*:*:123456789012:*", 1)],
            ),
            resolved_request("user"),
            check(None),
        )
        .await;

        assert_eq!(result, AuthorizationResult::Allow);
    }

    #[tokio::test]
    async fn enforce_mode_allows_seeded_admin_style_inline_policy_for_create_queue() {
        let result = authorize(
            &service(
                AuthorizationMode::Enforce,
                [inline_policy("Allow", "*", "arn:aws:*:*:123456789012:*", 1)],
            ),
            resolved_request("user"),
            auth_check(
                "sqs:CreateQueue",
                "arn:aws:sqs:us-east-1:123456789012:*",
                None,
            ),
        )
        .await;

        assert_eq!(result, AuthorizationResult::Allow);
    }
}
