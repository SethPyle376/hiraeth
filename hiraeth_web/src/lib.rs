use std::net::SocketAddr;

use askama::Template;
use axum::{
    Router,
    extract::State,
    http::header::{CACHE_CONTROL, HeaderValue},
    response::Html,
    routing::{get, get_service},
};
use hiraeth_store_sqlx::{SqliteIamStore, SqliteSqsStore};
use tokio::net::TcpListener;
use tower_http::{
    compression::CompressionLayer,
    services::{ServeDir, ServeFile},
    set_header::SetResponseHeaderLayer,
};

mod components;
mod error;
mod iam;
mod sqs;
mod templates;

use crate::{error::WebError, templates::HomeTemplate};

const APP_JS_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/app.js");
const APP_CSS_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/app.css");
const VENDOR_ASSETS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/vendor");
const APP_ASSET_CACHE_CONTROL: &str = "public, max-age=3600";
const VENDOR_ASSET_CACHE_CONTROL: &str = "public, max-age=31536000, immutable";

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
        .route_service(
            "/assets/app.js",
            get_service(ServeFile::new(APP_JS_PATH)).layer(SetResponseHeaderLayer::overriding(
                CACHE_CONTROL,
                HeaderValue::from_static(APP_ASSET_CACHE_CONTROL),
            )),
        )
        .route_service(
            "/assets/app.css",
            get_service(ServeFile::new(APP_CSS_PATH)).layer(SetResponseHeaderLayer::overriding(
                CACHE_CONTROL,
                HeaderValue::from_static(APP_ASSET_CACHE_CONTROL),
            )),
        )
        .nest_service(
            "/assets/vendor",
            get_service(ServeDir::new(VENDOR_ASSETS_DIR)).layer(
                SetResponseHeaderLayer::overriding(
                    CACHE_CONTROL,
                    HeaderValue::from_static(VENDOR_ASSET_CACHE_CONTROL),
                ),
            ),
        )
        .nest("/iam", iam::router())
        .nest("/sqs", sqs::router())
        .layer(CompressionLayer::new())
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
