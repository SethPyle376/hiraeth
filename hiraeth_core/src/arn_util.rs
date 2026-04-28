pub fn user_arn(account_id: &str, path: &str, user_name: &str) -> String {
    format!(
        "arn:aws:iam::{account_id}:user{}{user_name}",
        normalize_user_path(path)
    )
}

pub fn policy_arn(account_id: &str, path: &str, policy_name: &str) -> String {
    format!(
        "arn:aws:iam::{account_id}:user{}{policy_name}",
        normalize_user_path(path)
    )
}

pub fn normalize_user_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed == "/" {
        "/".to_string()
    } else {
        let trimmed = trimmed.trim_matches('/');
        format!("/{trimmed}/")
    }
}
