use hiraeth_core::ApiError;
use hiraeth_http::IncomingRequest;
use hiraeth_store::{StoreError, iam::AccessKeyStore};

mod sig_v4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    MissingAuthorizationHeader,
    InvalidAuthorizationHeader(String),
    MissingSignedHeader(String),
    InvalidSignature,
    SecretKeyNotFound,
    KeyStoreError(StoreError),
}

impl From<AuthError> for ApiError {
    fn from(value: AuthError) -> ApiError {
        match value {
            AuthError::MissingAuthorizationHeader => {
                ApiError::NotAuthenticated("Missing Authorization header".to_string())
            }
            AuthError::InvalidAuthorizationHeader(msg) => {
                ApiError::NotAuthenticated(format!("Invalid Authorization header: {}", msg))
            }
            AuthError::MissingSignedHeader(header) => {
                ApiError::NotAuthenticated(format!("Missing signed header: {}", header))
            }
            AuthError::InvalidSignature => {
                ApiError::NotAuthenticated("Invalid signature".to_string())
            }
            AuthError::SecretKeyNotFound => {
                ApiError::NotAuthenticated("Secret key not found for access key".to_string())
            }
            AuthError::KeyStoreError(e) => {
                ApiError::InternalServerError(format!("Access key store error: {:?}", e))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedRequest {
    pub request: IncomingRequest,
    pub service: String,
    pub region: String,
    pub auth_context: AuthContext,
    pub date: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthContext {
    pub access_key: String,
    pub principal_id: i64,
}

/// Authenticates an incoming request with SigV4 and attaches the authenticated
/// request context needed by IAM identity resolution.
pub async fn authenticate_request<S: AccessKeyStore>(
    request: IncomingRequest,
    store: &S,
) -> Result<AuthenticatedRequest, AuthError> {
    let (sig_v4_params, access_key) = sig_v4::authenticate_request(&request, store).await?;
    let request_timestamp = request
        .headers
        .get("x-amz-date")
        .ok_or(AuthError::MissingSignedHeader("x-amz-date".to_string()))?;
    let date = chrono::NaiveDateTime::parse_from_str(request_timestamp, "%Y%m%dT%H%M%SZ")
        .map_err(|_| AuthError::InvalidAuthorizationHeader("Date format incorrect".to_string()))?
        .and_utc();

    let auth_context = AuthContext {
        access_key: access_key.key_id.clone(),
        principal_id: access_key.principal_id,
    };

    Ok(AuthenticatedRequest {
        request,
        service: sig_v4_params.service,
        region: sig_v4_params.region,
        auth_context,
        date,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use hiraeth_store::iam::{AccessKey, InMemoryAccessKeyStore};

    use super::authenticate_request;
    use hiraeth_http::IncomingRequest;

    fn access_key_store() -> InMemoryAccessKeyStore {
        InMemoryAccessKeyStore::new([AccessKey {
            key_id: "AKIAIOSFODNN7EXAMPLE".to_string(),
            principal_id: 1,
            secret_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".to_string(),
            created_at: Utc
                .with_ymd_and_hms(2026, 3, 30, 12, 0, 0)
                .unwrap()
                .naive_utc(),
        }])
    }

    #[tokio::test]
    async fn authenticate_request_returns_authenticated_request_context() {
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());
        headers.insert(
            "host".to_string(),
            "sqs.us-east-1.amazonaws.com".to_string(),
        );
        headers.insert("x-amz-date".to_string(), "20260330T120000Z".to_string());
        headers.insert(
            "authorization".to_string(),
            "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20260330/us-east-1/sqs/aws4_request, SignedHeaders=content-type;host;x-amz-date, Signature=ffff699a5016d0166b23b26521afd5147ba0d923ca7ec1153d95db81e1cbce6c".to_string(),
        );

        let request = IncomingRequest {
            host: "sqs.us-east-1.amazonaws.com".to_string(),
            method: "POST".to_string(),
            path: "/hello".to_string(),
            query: Some("b=two&a=one".to_string()),
            headers,
            body: "hello world".to_string().into_bytes(),
        };

        let access_key_store = access_key_store();
        let authenticated = authenticate_request(request, &access_key_store)
            .await
            .expect("request should authenticate");

        assert_eq!(authenticated.service, "sqs");
        assert_eq!(authenticated.region, "us-east-1");
        assert_eq!(
            authenticated.auth_context.access_key,
            "AKIAIOSFODNN7EXAMPLE"
        );
        assert_eq!(authenticated.auth_context.principal_id, 1);
        assert_eq!(
            authenticated.date,
            Utc.with_ymd_and_hms(2026, 3, 30, 12, 0, 0).unwrap()
        );
        assert_eq!(authenticated.request.method, "POST");
        assert_eq!(authenticated.request.path, "/hello");
        assert_eq!(authenticated.request.query, Some("b=two&a=one".to_string()));
    }
}
