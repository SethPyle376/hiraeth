use hiraeth_core::{ResolvedRequest, arn_util, auth::AuthorizationCheck, parse_aws_query_params};
use hiraeth_store::IamStore;

use crate::error::IamError;

pub(crate) async fn resolve_authorization<S: IamStore + Send + Sync>(
    authorization_action: &str,
    request: &ResolvedRequest,
    store: &S,
) -> Result<AuthorizationCheck, IamError> {
    let params = parse_aws_query_params(&request.request)
        .map_err(|e| IamError::BadRequest(e.to_string()))?;
    let account_id = &request.auth_context.principal.account_id;

    match authorization_action {
        "iam:CreateUser" => {
            let user_name = params
                .get("UserName")
                .ok_or_else(|| IamError::BadRequest("UserName is required".to_string()))?;
            let path = params.get("Path").unwrap_or("/");
            let path = arn_util::normalize_user_path(path);
            Ok(AuthorizationCheck {
                action: authorization_action.to_string(),
                resource: arn_util::user_arn(account_id, &path, user_name),
                resource_policy: None,
            })
        }
        "iam:GetUser" => {
            let user_name = params.get("UserName");
            let principal = if let Some(name) = user_name {
                store
                    .get_principal_by_identity(account_id, "user", name)
                    .await
                    .map_err(IamError::from)?
                    .ok_or_else(|| {
                        IamError::NoSuchEntity(format!("User with name '{name}' not found"))
                    })?
            } else {
                store
                    .get_principal_by_identity(
                        account_id,
                        "user",
                        &request.auth_context.principal.name,
                    )
                    .await
                    .map_err(IamError::from)?
                    .ok_or_else(|| {
                        IamError::NoSuchEntity(format!(
                            "User with name '{}' not found",
                            request.auth_context.principal.name
                        ))
                    })?
            };
            Ok(AuthorizationCheck {
                action: authorization_action.to_string(),
                resource: arn_util::user_arn(
                    &principal.account_id,
                    &principal.path,
                    &principal.name,
                ),
                resource_policy: None,
            })
        }
        "iam:DeleteUser" => {
            let user_name = params
                .get("UserName")
                .ok_or_else(|| IamError::BadRequest("UserName is required".to_string()))?;
            let principal = store
                .get_principal_by_identity(account_id, "user", user_name)
                .await
                .map_err(IamError::from)?
                .ok_or_else(|| {
                    IamError::NoSuchEntity(format!("User with name '{user_name}' not found"))
                })?;
            Ok(AuthorizationCheck {
                action: authorization_action.to_string(),
                resource: arn_util::user_arn(
                    &principal.account_id,
                    &principal.path,
                    &principal.name,
                ),
                resource_policy: None,
            })
        }
        "iam:CreateAccessKey" | "iam:ListAccessKeys" | "iam:ListAttachedUserPolicies" => {
            let user_name = params.get("UserName");
            let principal = match user_name {
                Some(name) if !name.trim().is_empty() => store
                    .get_principal_by_identity(account_id, "user", name)
                    .await
                    .map_err(IamError::from)?
                    .ok_or_else(|| IamError::NoSuchEntity(format!("User {name} does not exist")))?,
                _ => {
                    if request.auth_context.principal.kind == "user" {
                        request.auth_context.principal.clone()
                    } else {
                        return Err(IamError::BadRequest(
                            "UserName is required when the caller is not an IAM user".to_string(),
                        ));
                    }
                }
            };
            Ok(AuthorizationCheck {
                action: authorization_action.to_string(),
                resource: arn_util::user_arn(
                    &principal.account_id,
                    &principal.path,
                    &principal.name,
                ),
                resource_policy: None,
            })
        }
        "iam:PutUserPolicy"
        | "iam:GetUserPolicy"
        | "iam:AttachUserPolicy"
        | "iam:DetachUserPolicy" => {
            let user_name = params
                .get("UserName")
                .ok_or_else(|| IamError::BadRequest("UserName is required".to_string()))?;
            let principal = store
                .get_principal_by_identity(account_id, "user", user_name)
                .await
                .map_err(IamError::from)?
                .ok_or_else(|| {
                    IamError::NoSuchEntity(format!("User {user_name} does not exist"))
                })?;
            Ok(AuthorizationCheck {
                action: authorization_action.to_string(),
                resource: arn_util::user_arn(
                    &principal.account_id,
                    &principal.path,
                    &principal.name,
                ),
                resource_policy: None,
            })
        }
        "iam:CreatePolicy" => {
            let policy_name = params
                .get("PolicyName")
                .ok_or_else(|| IamError::BadRequest("PolicyName is required".to_string()))?;
            let path = params.get("Path").unwrap_or("/");
            let path = arn_util::normalize_user_path(path);
            Ok(AuthorizationCheck {
                action: authorization_action.to_string(),
                resource: arn_util::policy_arn(account_id, &path, policy_name),
                resource_policy: None,
            })
        }
        "iam:GetPolicy" | "iam:DeletePolicy" | "iam:GetPolicyVersion" => {
            let policy_arn = params
                .get("PolicyArn")
                .ok_or_else(|| IamError::BadRequest("PolicyArn is required".to_string()))?;
            Ok(AuthorizationCheck {
                action: authorization_action.to_string(),
                resource: policy_arn.to_string(),
                resource_policy: None,
            })
        }
        _ => Err(IamError::UnsupportedOperation(
            authorization_action.to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_core::{AuthContext, ResolvedRequest, get_query_request_action_name};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::principal::Principal;

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

    #[test]
    fn resolves_create_user_action_name() {
        let request = resolved_request(b"Action=CreateUser&Version=2010-05-08&UserName=alice");

        let action = get_query_request_action_name(&request)
            .expect("action query should parse")
            .expect("action should be present");

        assert_eq!(action, "CreateUser");
    }

    #[test]
    fn returns_none_when_action_parameter_is_missing() {
        let request = resolved_request(b"Version=2010-05-08&UserName=alice");

        let action = get_query_request_action_name(&request).expect("query should parse");
        assert_eq!(action, None);
    }
}
