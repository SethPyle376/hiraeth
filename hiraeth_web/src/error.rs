use askama::Template;
use axum::{
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};
use hiraeth_store::StoreError;

use crate::templates::ErrorTemplate;

#[derive(Debug)]
pub(crate) struct WebError {
    status: StatusCode,
    title: &'static str,
    message: String,
}

impl From<StoreError> for WebError {
    fn from(value: StoreError) -> Self {
        let (status, title) = match &value {
            StoreError::NotFound(_) => (StatusCode::NOT_FOUND, "Not found"),
            StoreError::Conflict(_) => (StatusCode::CONFLICT, "Conflict"),
            StoreError::StorageFailure(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Storage error"),
        };

        Self {
            status,
            title,
            message: value.to_string(),
        }
    }
}

impl From<askama::Error> for WebError {
    fn from(value: askama::Error) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            title: "Template error",
            message: value.to_string(),
        }
    }
}

impl IntoResponse for WebError {
    fn into_response(self) -> Response {
        let body = ErrorTemplate {
            status_code: self.status.as_u16(),
            title: self.title,
            message: &self.message,
        }
        .render()
        .unwrap_or(self.message);

        (self.status, Html(body)).into_response()
    }
}
