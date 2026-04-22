use hiraeth_http::IncomingRequest;
use hiraeth_store::iam::Principal;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRequest {
    pub request: IncomingRequest,
    pub service: String,
    pub region: String,
    pub auth_context: AuthContext,
    pub date: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthContext {
    pub access_key: String,
    pub principal: Principal,
}
