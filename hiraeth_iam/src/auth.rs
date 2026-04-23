use hiraeth_core::{ResolvedRequest, parse_aws_query_params, parse_aws_query_request};
use serde::Deserialize;

use crate::error::IamError;

const IAM_API_VERSION: &str = "2010-05-08";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IamAction {
    CreateUser,
}

impl IamAction {
    pub(crate) fn from_request(request: &ResolvedRequest) -> Result<Self, IamError> {
        let params = parse_aws_query_params(&request.request)?;
        let action = params
            .get("Action")
            .ok_or_else(|| IamError::BadRequest("Missing Action parameter".to_string()))?;
        let version = params
            .get("Version")
            .ok_or_else(|| IamError::BadRequest("Missing Version parameter".to_string()))?;

        if version != IAM_API_VERSION {
            return Err(IamError::BadRequest(format!(
                "Unsupported Version parameter '{}'",
                version
            )));
        }

        match action {
            "CreateUser" => Ok(Self::CreateUser),
            _ => Err(IamError::UnsupportedOperation(action.to_string())),
        }
    }

    pub(crate) fn authorization_action(self) -> &'static str {
        match self {
            Self::CreateUser => "iam:CreateUser",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CreateUserAuthRequest {
    user_name: String,
    #[serde(default = "default_user_path")]
    path: String,
}

pub(crate) fn get_action_for_request(request: &ResolvedRequest) -> Result<String, IamError> {
    Ok(IamAction::from_request(request)?
        .authorization_action()
        .to_string())
}

pub(crate) fn get_resource_for_action(
    action: IamAction,
    request: &ResolvedRequest,
) -> Result<String, IamError> {
    match action {
        IamAction::CreateUser => {
            let create_user_request: CreateUserAuthRequest =
                parse_aws_query_request(&request.request)?;

            Ok(user_arn(
                &request.auth_context.principal.account_id,
                &create_user_request.path,
                &create_user_request.user_name,
            ))
        }
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
    use hiraeth_core::{AuthContext, ResolvedRequest};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::principal::Principal;

    use super::{IamAction, get_action_for_request, get_resource_for_action};

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

    #[test]
    fn maps_create_user_query_request_to_iam_action() {
        let request = resolved_request(b"Action=CreateUser&Version=2010-05-08&UserName=alice");

        let action = get_action_for_request(&request).expect("action should resolve");

        assert_eq!(action, "iam:CreateUser");
    }

    #[test]
    fn resolves_create_user_resource_with_default_path() {
        let request = resolved_request(b"Action=CreateUser&Version=2010-05-08&UserName=alice");

        let resource = get_resource_for_action(IamAction::CreateUser, &request)
            .expect("resource should resolve");

        assert_eq!(resource, "arn:aws:iam::123456789012:user/alice");
    }

    #[test]
    fn resolves_create_user_resource_with_custom_path() {
        let request = resolved_request(
            b"Action=CreateUser&Version=2010-05-08&UserName=alice&Path=%2Fengineering%2Fdev%2F",
        );

        let resource = get_resource_for_action(IamAction::CreateUser, &request)
            .expect("resource should resolve");

        assert_eq!(
            resource,
            "arn:aws:iam::123456789012:user/engineering/dev/alice"
        );
    }

    #[test]
    fn rejects_missing_version_parameter() {
        let request = resolved_request(b"Action=CreateUser&UserName=alice");

        let error = IamAction::from_request(&request).expect_err("missing version should fail");

        assert_eq!(
            error,
            IamError::BadRequest("Missing Version parameter".to_string())
        );
    }

    #[test]
    fn rejects_unknown_iam_action() {
        let request = resolved_request(b"Action=DeleteUser&Version=2010-05-08&UserName=alice");

        let error = IamAction::from_request(&request).expect_err("unknown action should fail");

        assert_eq!(
            error,
            IamError::UnsupportedOperation("DeleteUser".to_string())
        );
    }

    use crate::error::IamError;
}
