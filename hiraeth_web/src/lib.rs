use std::net::SocketAddr;

use askama::Template;
use axum::{Router, extract::State, response::Html, routing::get};
use hiraeth_store_sqlx::SqliteSqsStore;
use tokio::net::TcpListener;

mod error;
mod sqs;
mod templates;

use crate::{error::WebError, templates::HomeTemplate};

#[derive(Clone)]
pub struct WebState {
    pub sqs_store: SqliteSqsStore,
}

impl WebState {
    pub fn new(sqs_store: SqliteSqsStore) -> Self {
        Self { sqs_store }
    }
}

pub fn router(state: WebState) -> Router {
    Router::new()
        .route("/", get(home))
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
