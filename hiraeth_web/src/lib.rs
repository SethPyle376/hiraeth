use std::net::SocketAddr;

use askama::Template;
use axum::{
    Router,
    body::Body,
    extract::State,
    http::{
        HeaderValue, Response,
        header::{CACHE_CONTROL, CONTENT_TYPE},
    },
    response::{Html, IntoResponse},
    routing::get,
};
use hiraeth_store_sqlx::SqliteTraceStore;
use hiraeth_store_sqlx::{SqliteIamStore, SqliteSnsStore, SqliteSqsStore};
use tokio::net::TcpListener;
use tower_http::compression::CompressionLayer;

mod components;
mod error;
mod iam;
mod sns;
mod sqs;
mod templates;
mod traces;

use crate::{error::WebError, templates::HomeTemplate};

const APP_JS_BYTES: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/app.js"));
const APP_CSS_BYTES: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/app.css"));
const FAVICON_BYTES: &[u8] =
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/favicon.svg"));
const HTMX_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/vendor/htmx.min.js"
));
const APP_ASSET_CACHE_CONTROL: &str = "public, max-age=0, must-revalidate";
const VENDOR_ASSET_CACHE_CONTROL: &str = "public, max-age=31536000, immutable";

#[derive(Clone)]
pub struct WebState {
    pub iam_store: SqliteIamStore,
    pub sqs_store: SqliteSqsStore,
    pub sns_store: SqliteSnsStore,
    pub trace_store: SqliteTraceStore,
    pub aws_endpoint_url: String,
}

impl WebState {
    pub fn new(
        iam_store: SqliteIamStore,
        sqs_store: SqliteSqsStore,
        sns_store: SqliteSnsStore,
        trace_store: SqliteTraceStore,
    ) -> Self {
        Self {
            iam_store,
            sqs_store,
            sns_store,
            trace_store,
            aws_endpoint_url: "http://localhost:4566".to_string(),
        }
    }

    pub fn with_aws_endpoint_url(mut self, aws_endpoint_url: impl Into<String>) -> Self {
        self.aws_endpoint_url = aws_endpoint_url.into().trim_end_matches('/').to_string();
        self
    }
}

pub fn router(state: WebState) -> Router {
    let asset_router = Router::new()
        .route("/assets/app.js", get(app_js))
        .route("/assets/app.css", get(app_css))
        .route("/assets/vendor/htmx.min.js", get(htmx_js))
        .route("/favicon.svg", get(favicon_svg))
        .route("/favicon.ico", get(favicon_ico))
        .layer(CompressionLayer::new());

    Router::new()
        .route("/", get(home))
        .merge(asset_router)
        .nest("/iam", iam::router())
        .nest("/sns", sns::router())
        .nest("/sqs", sqs::router())
        .nest("/traces", traces::router())
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

async fn app_js() -> Response<Body> {
    static_asset_response(
        APP_JS_BYTES,
        "application/javascript; charset=utf-8",
        APP_ASSET_CACHE_CONTROL,
    )
}

async fn app_css() -> Response<Body> {
    static_asset_response(
        APP_CSS_BYTES,
        "text/css; charset=utf-8",
        APP_ASSET_CACHE_CONTROL,
    )
}

async fn htmx_js() -> Response<Body> {
    static_asset_response(
        HTMX_BYTES,
        "application/javascript; charset=utf-8",
        VENDOR_ASSET_CACHE_CONTROL,
    )
}

async fn favicon_svg() -> Response<Body> {
    static_asset_response(FAVICON_BYTES, "image/svg+xml", APP_ASSET_CACHE_CONTROL)
}

async fn favicon_ico() -> Response<Body> {
    static_asset_response(FAVICON_BYTES, "image/svg+xml", APP_ASSET_CACHE_CONTROL)
}

fn static_asset_response(
    bytes: &'static [u8],
    content_type: &'static str,
    cache_control: &'static str,
) -> Response<Body> {
    (
        [
            (CONTENT_TYPE, HeaderValue::from_static(content_type)),
            (CACHE_CONTROL, HeaderValue::from_static(cache_control)),
        ],
        bytes,
    )
        .into_response()
}
