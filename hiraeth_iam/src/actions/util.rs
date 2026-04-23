use chrono::SecondsFormat;
use hiraeth_store::iam::Principal;
use serde::Serialize;

pub(super) const IAM_XMLNS: &str = "https://iam.amazonaws.com/doc/2010-05-08/";

#[derive(Debug, Serialize)]
pub(super) struct ResponseMetadata {
    #[serde(rename = "RequestId")]
    pub request_id: String,
}

pub(super) fn response_metadata(request_id: impl Into<String>) -> ResponseMetadata {
    ResponseMetadata {
        request_id: request_id.into(),
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct IamUserXml {
    #[serde(rename = "Path")]
    pub path: String,
    #[serde(rename = "UserName")]
    pub user_name: String,
    #[serde(rename = "UserId")]
    pub user_id: String,
    #[serde(rename = "Arn")]
    pub arn: String,
    #[serde(rename = "CreateDate")]
    pub create_date: String,
}

impl From<Principal> for IamUserXml {
    fn from(principal: Principal) -> Self {
        IamUserXml {
            path: principal.path.clone(),
            user_name: principal.name.clone(),
            user_id: principal.user_id.clone(),
            arn: user_arn(&principal.account_id, &principal.path, &principal.name),
            create_date: principal
                .created_at
                .and_utc()
                .to_rfc3339_opts(SecondsFormat::Secs, true),
        }
    }
}

pub(super) fn user_arn(account_id: &str, path: &str, user_name: &str) -> String {
    format!(
        "arn:aws:iam::{account_id}:user{}{user_name}",
        normalize_user_path(path)
    )
}

pub(super) fn normalize_user_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed == "/" {
        "/".to_string()
    } else {
        let trimmed = trimmed.trim_matches('/');
        format!("/{trimmed}/")
    }
}

pub(super) fn default_user_path() -> String {
    "/".to_string()
}
