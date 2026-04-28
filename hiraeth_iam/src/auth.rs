use hiraeth_core::{ResolvedRequest, parse_aws_query_params};

use crate::error::IamError;

const IAM_API_VERSION: &str = "2010-05-08";

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_core::{AuthContext, ResolvedRequest, get_query_request_action_name};
    use hiraeth_http::IncomingRequest;
    use hiraeth_store::principal::Principal;

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
