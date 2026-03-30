mod sig_v4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    MissingAuthorizationHeader,
    InvalidAuthorizationHeader,
    MissingSignedHeader(String),
    InvalidSignature,
}

pub enum AuthContext {
    Authenticated {
        access_key: String,
        secret_key: String,
        region: String,
        service: String,
    },
    Unauthenticated,
}
