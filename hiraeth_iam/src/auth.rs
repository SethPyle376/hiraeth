use hiraeth_core::{ResolvedRequest, parse_aws_query_params};

use crate::error::IamError;

const IAM_API_VERSION: &str = "2010-05-08";

pub(crate) fn get_action_name_for_request(request: &ResolvedRequest) -> Result<String, IamError> {
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

    Ok(action.to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_core::{AuthContext, ResolvedRequest};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::principal::Principal;

    use super::get_action_name_for_request;
    use crate::error::IamError;

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
    fn resolves_create_user_action_name() {
        let request = resolved_request(b"Action=CreateUser&Version=2010-05-08&UserName=alice");

        let action = get_action_name_for_request(&request).expect("action should resolve");

        assert_eq!(action, "CreateUser");
    }

    #[test]
    fn rejects_missing_version_parameter() {
        let request = resolved_request(b"Action=CreateUser&UserName=alice");

        let error = get_action_name_for_request(&request).expect_err("missing version should fail");

        assert_eq!(
            error,
            IamError::BadRequest("Missing Version parameter".to_string())
        );
    }
}
