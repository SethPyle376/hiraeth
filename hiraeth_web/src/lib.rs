use std::net::SocketAddr;

use askama::Template;
use axum::{Router, extract::State, response::Html, routing::get};
use hiraeth_store_sqlx::{SqliteIamStore, SqliteSqsStore};
use tokio::net::TcpListener;

mod error;
mod iam;
mod sqs;
mod templates;

use crate::{error::WebError, templates::HomeTemplate};

#[derive(Clone)]
pub struct WebState {
    pub iam_store: SqliteIamStore,
    pub sqs_store: SqliteSqsStore,
    pub aws_endpoint_url: String,
}

impl WebState {
    pub fn new(iam_store: SqliteIamStore, sqs_store: SqliteSqsStore) -> Self {
        Self {
            iam_store,
            sqs_store,
            aws_endpoint_url: "http://localhost:4566".to_string(),
        }
    }

    pub fn with_aws_endpoint_url(mut self, aws_endpoint_url: impl Into<String>) -> Self {
        self.aws_endpoint_url = aws_endpoint_url.into().trim_end_matches('/').to_string();
        self
    }
}

pub fn router(state: WebState) -> Router {
    Router::new()
        .route("/", get(home))
        .nest("/iam", iam::router())
        .nest("/sqs", sqs::router())
        .with_state(state)
}

pub async fn serve(addr: SocketAddr, state: WebState) -> anyhow::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, router(state)).await?;
    Ok(())
}

async fn home(State(_state): State<WebState>) -> Result<Html<String>, WebError> {
    Ok(Html(HomeTemplate.render()?))
}
