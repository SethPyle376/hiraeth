use serde::{Deserialize, Deserializer, Serialize, de::Error};

use crate::auth::util::OneOrMany;

#[derive(Debug, PartialEq, Eq)]
pub enum PolicyPrincipal {
    Any,
    Account(String),
    User {
        account_id: String,
        user_name: String,
    },
    Role {
        account_id: String,
        role_name: String,
    },
    AssumedRole {
        account_id: String,
        role_name: String,
        session_name: String,
    },
    Service(String),
}

pub(crate) fn deserialize_principals<'de, D>(
    deserializer: D,
) -> Result<Vec<PolicyPrincipal>, D::Error>
where
    D: Deserializer<'de>,
{
    let wire = PrincipalWire::deserialize(deserializer)?;

    match wire {
        PrincipalWire::Any(value) => {
            if value == "*" {
                Ok(vec![PolicyPrincipal::Any])
            } else {
                Err(Error::custom(format!(
                    "expected '*' for Any principal, got '{}'",
                    value
                )))
            }
        }
        PrincipalWire::Map(principal_map_wire) => {
            let mut principals = Vec::new();

            for principal in principal_map_wire
                .aws
                .into_iter()
                .flat_map(OneOrMany::into_vec)
            {
                principals.push(parse_principal(&principal).map_err(|err| {
                    Error::custom(format!("failed to parse AWS principal: {}", err))
                })?);
            }

            for principal in principal_map_wire
                .service
                .into_iter()
                .flat_map(OneOrMany::into_vec)
            {
                principals.push(PolicyPrincipal::Service(principal));
            }

            if principals.is_empty() {
                Err(Error::custom(
                    "principal map must contain at least one supported principal",
                ))
            } else {
                Ok(principals)
            }
        }
    }
}

fn parse_principal(value: &str) -> Result<PolicyPrincipal, String> {
    if value == "*" {
        return Ok(PolicyPrincipal::Any);
    }

    if is_account_id(value) {
        return Ok(PolicyPrincipal::Account(value.to_string()));
    }

    let parts = value.splitn(6, ':').collect::<Vec<&str>>();

    if parts.len() == 6 && parts[0] == "arn" {
        let (service, account_id, resource) = (parts[2], parts[4], parts[5]);

        if !is_account_id(account_id) {
            return Err(format!("invalid account id in principal ARN: '{}'", value));
        }

        match service {
            "iam" => parse_iam_principal(account_id, resource),
            "sts" => parse_sts_principal(account_id, resource),
            _ => Err(format!(
                "unsupported service in principal ARN: '{}'",
                service
            )),
        }
    } else {
        Err(format!("invalid principal format: '{}'", value))
    }
}

fn parse_iam_principal(account_id: &str, resource: &str) -> Result<PolicyPrincipal, String> {
    if resource == "root" {
        Ok(PolicyPrincipal::Account(account_id.to_string()))
    } else if let Some(user_name) = resource.strip_prefix("user/") {
        Ok(PolicyPrincipal::User {
            account_id: account_id.to_string(),
            user_name: user_name.to_string(),
        })
    } else if let Some(role_name) = resource.strip_prefix("role/") {
        Ok(PolicyPrincipal::Role {
            account_id: account_id.to_string(),
            role_name: role_name.to_string(),
        })
    } else {
        Err(format!(
            "unsupported IAM resource in principal ARN: '{}'",
            resource
        ))
    }
}

fn parse_sts_principal(account_id: &str, resource: &str) -> Result<PolicyPrincipal, String> {
    if let Some(session) = resource.strip_prefix("assumed-role/") {
        if let Some((role_name, session_name)) = session.split_once('/') {
            Ok(PolicyPrincipal::AssumedRole {
                account_id: account_id.to_string(),
                role_name: role_name.to_string(),
                session_name: session_name.to_string(),
            })
        } else {
            Err(format!("invalid assumed role format: '{}'", resource))
        }
    } else {
        Err(format!(
            "unsupported STS resource in principal ARN: '{}'",
            resource
        ))
    }
}

fn is_account_id(value: &str) -> bool {
    value.len() == 12 && value.bytes().all(|byte| byte.is_ascii_digit())
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum PrincipalWire {
    Any(String),
    Map(PrincipalMapWire),
}

#[derive(Debug, Serialize, Deserialize)]
struct PrincipalMapWire {
    #[serde(rename = "AWS")]
    aws: Option<OneOrMany<String>>,
    #[serde(rename = "Service")]
    service: Option<OneOrMany<String>>,
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::{PolicyPrincipal, deserialize_principals};

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct PrincipalFixture {
        #[serde(deserialize_with = "deserialize_principals")]
        principal: Vec<PolicyPrincipal>,
    }

    fn deserialize_principal(json: &str) -> serde_json::Result<Vec<PolicyPrincipal>> {
        serde_json::from_str::<PrincipalFixture>(&format!(r#"{{"Principal":{json}}}"#))
            .map(|fixture| fixture.principal)
    }

    #[test]
    fn deserializes_wildcard_principal() {
        let principals = deserialize_principal(r#""*""#).expect("principal should deserialize");

        assert_eq!(principals, vec![PolicyPrincipal::Any]);
    }

    #[test]
    fn rejects_non_wildcard_string_principal() {
        let result = deserialize_principal(r#""not-a-wildcard""#);

        assert!(result.is_err());
    }

    #[test]
    fn deserializes_aws_wildcard_principal() {
        let principals =
            deserialize_principal(r#"{"AWS":"*"}"#).expect("principal should deserialize");

        assert_eq!(principals, vec![PolicyPrincipal::Any]);
    }

    #[test]
    fn deserializes_account_id_principal() {
        let principals = deserialize_principal(r#"{"AWS":"123456789012"}"#)
            .expect("principal should deserialize");

        assert_eq!(
            principals,
            vec![PolicyPrincipal::Account("123456789012".to_string())]
        );
    }

    #[test]
    fn deserializes_root_arn_as_account_principal() {
        let principals = deserialize_principal(r#"{"AWS":"arn:aws:iam::123456789012:root"}"#)
            .expect("principal should deserialize");

        assert_eq!(
            principals,
            vec![PolicyPrincipal::Account("123456789012".to_string())]
        );
    }

    #[test]
    fn deserializes_iam_user_principal() {
        let principals = deserialize_principal(r#"{"AWS":"arn:aws:iam::123456789012:user/alice"}"#)
            .expect("principal should deserialize");

        assert_eq!(
            principals,
            vec![PolicyPrincipal::User {
                account_id: "123456789012".to_string(),
                user_name: "alice".to_string(),
            }]
        );
    }

    #[test]
    fn deserializes_path_qualified_iam_role_principal() {
        let principals = deserialize_principal(
            r#"{"AWS":"arn:aws:iam::123456789012:role/service/team/app-worker"}"#,
        )
        .expect("principal should deserialize");

        assert_eq!(
            principals,
            vec![PolicyPrincipal::Role {
                account_id: "123456789012".to_string(),
                role_name: "service/team/app-worker".to_string(),
            }]
        );
    }

    #[test]
    fn deserializes_assumed_role_principal() {
        let principals = deserialize_principal(
            r#"{"AWS":"arn:aws:sts::123456789012:assumed-role/app-worker/session-name"}"#,
        )
        .expect("principal should deserialize");

        assert_eq!(
            principals,
            vec![PolicyPrincipal::AssumedRole {
                account_id: "123456789012".to_string(),
                role_name: "app-worker".to_string(),
                session_name: "session-name".to_string(),
            }]
        );
    }

    #[test]
    fn deserializes_service_principal() {
        let principals = deserialize_principal(r#"{"Service":"s3.amazonaws.com"}"#)
            .expect("principal should deserialize");

        assert_eq!(
            principals,
            vec![PolicyPrincipal::Service("s3.amazonaws.com".to_string())]
        );
    }

    #[test]
    fn deserializes_mixed_principal_map() {
        let principals = deserialize_principal(
            r#"{"AWS":["123456789012","arn:aws:iam::210987654321:user/bob"],"Service":["sns.amazonaws.com","events.amazonaws.com"]}"#,
        )
        .expect("principal should deserialize");

        assert_eq!(
            principals,
            vec![
                PolicyPrincipal::Account("123456789012".to_string()),
                PolicyPrincipal::User {
                    account_id: "210987654321".to_string(),
                    user_name: "bob".to_string(),
                },
                PolicyPrincipal::Service("sns.amazonaws.com".to_string()),
                PolicyPrincipal::Service("events.amazonaws.com".to_string()),
            ]
        );
    }

    #[test]
    fn rejects_empty_principal_map() {
        let result = deserialize_principal(r#"{}"#);

        assert!(result.is_err());
    }

    #[test]
    fn rejects_invalid_aws_principal() {
        let result = deserialize_principal(r#"{"AWS":"not-an-account-or-arn"}"#);

        assert!(result.is_err());
    }
}
