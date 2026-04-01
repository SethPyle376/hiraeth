#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiError {
    NotFound,
    InternalServerError(String),
    BadRequest(String),
    NotAuthorized(String),
    NotAuthenticated(String),
}

impl ApiError {
    pub fn status_code(&self) -> u16 {
        match self {
            ApiError::NotFound => 404,
            ApiError::InternalServerError(_) => 500,
            ApiError::BadRequest(_) => 400,
            ApiError::NotAuthorized(_) => 403,
            ApiError::NotAuthenticated(_) => 401,
        }
    }

    pub fn message(&self) -> String {
        match self {
            ApiError::NotFound => "Not Found".to_string(),
            ApiError::InternalServerError(msg) => format!("Internal Server Error: {}", msg),
            ApiError::BadRequest(msg) => format!("Bad Request: {}", msg),
            ApiError::NotAuthorized(msg) => format!("Not Authorized: {}", msg),
            ApiError::NotAuthenticated(msg) => format!("Not Authenticated: {}", msg),
        }
    }
}
