use askama::Template;
use axum::{
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};
use hiraeth_store::StoreError;

use crate::templates::ErrorTemplate;

#[derive(Debug)]
pub(crate) struct WebError {
    message: String,
}

impl From<StoreError> for WebError {
    fn from(value: StoreError) -> Self {
        Self {
            message: value.to_string(),
        }
    }
}

impl From<askama::Error> for WebError {
    fn from(value: askama::Error) -> Self {
        Self {
            message: value.to_string(),
        }
    }
}

impl IntoResponse for WebError {
    fn into_response(self) -> Response {
        let body = ErrorTemplate {
            message: &self.message,
        }
        .render()
        .unwrap_or_else(|_| self.message);

        (StatusCode::INTERNAL_SERVER_ERROR, Html(body)).into_response()
    }
}
