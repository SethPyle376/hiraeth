use hiraeth_http::IncomingRequest;

mod sig_v4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    MissingAuthorizationHeader,
    InvalidAuthorizationHeader,
    MissingSignedHeader(String),
    InvalidSignature,
}

pub struct ResolvedRequest {
    pub request: IncomingRequest,
    pub service: String,
    pub region: String,
    pub access_key: String,
    pub date: chrono::DateTime<chrono::Utc>,
}

/// Authenticates an incoming request with SigV4 and attaches the resolved
/// request context needed by downstream service handlers.
pub fn resolve_request(request: IncomingRequest) -> Result<ResolvedRequest, AuthError> {
    let sig_v4_params = sig_v4::authenticate_request(&request)?;
    let request_timestamp = request
        .headers
        .get("x-amz-date")
        .ok_or(AuthError::MissingSignedHeader("x-amz-date".to_string()))?;
    let date = chrono::NaiveDateTime::parse_from_str(request_timestamp, "%Y%m%dT%H%M%SZ")
        .map_err(|_| AuthError::InvalidAuthorizationHeader)?
        .and_utc();

    Ok(ResolvedRequest {
        request,
        service: sig_v4_params.service,
        region: sig_v4_params.region,
        access_key: sig_v4_params.access_key,
        date,
    })
}

trait AcessKeyStore {
    fn get_secret_key(&self, access_key: &str) -> Result<Option<String>, AuthError>;
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};

    use super::resolve_request;
    use hiraeth_http::IncomingRequest;

    #[test]
    fn resolve_request_returns_authenticated_request_context() {
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());
        headers.insert(
            "host".to_string(),
            "sqs.us-east-1.amazonaws.com".to_string(),
        );
        headers.insert("x-amz-date".to_string(), "20260330T120000Z".to_string());
        headers.insert(
            "authorization".to_string(),
            "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20260330/us-east-1/sqs/aws4_request,SignedHeaders=content-type;host;x-amz-date,Signature=ffff699a5016d0166b23b26521afd5147ba0d923ca7ec1153d95db81e1cbce6c".to_string(),
        );

        let request = IncomingRequest {
            method: "POST".to_string(),
            path: "/hello".to_string(),
            query: Some("b=two&a=one".to_string()),
            headers,
            body: "hello world".to_string().into_bytes(),
        };

        let resolved = resolve_request(request).expect("request should resolve");

        assert_eq!(resolved.service, "sqs");
        assert_eq!(resolved.region, "us-east-1");
        assert_eq!(resolved.access_key, "AKIAIOSFODNN7EXAMPLE");
        assert_eq!(
            resolved.date,
            Utc.with_ymd_and_hms(2026, 3, 30, 12, 0, 0).unwrap()
        );
        assert_eq!(resolved.request.method, "POST");
        assert_eq!(resolved.request.path, "/hello");
        assert_eq!(resolved.request.query, Some("b=two&a=one".to_string()));
    }
}
