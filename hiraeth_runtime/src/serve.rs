use std::{
    convert::Infallible,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

use http_body_util::Full;
use hyper::{
    Request,
    body::{Bytes, Incoming},
    server::conn::http1,
    service::service_fn,
};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;

use crate::app::{App, AppRequestOutcome};

static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

pub async fn serve(addr: SocketAddr, app: Arc<App>) -> anyhow::Result<()> {
    let listener = TcpListener::bind(addr).await?;

    loop {
        let (stream, peer_addr) = listener.accept().await?;
        let app = Arc::clone(&app);

        let service = service_fn(move |request| handle_request(Arc::clone(&app), request));
        tokio::spawn(async move {
            let io = TokioIo::new(stream);
            if let Err(e) = http1::Builder::new().serve_connection(io, service).await {
                eprintln!("Connection error from {}: {:?}", peer_addr, e);
            }
        });
    }
}

async fn handle_request(
    app: Arc<App>,
    request: Request<Incoming>,
) -> Result<hyper::Response<Full<Bytes>>, Infallible> {
    let request_id = NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    let started_at = Instant::now();

    let incoming_request = match hiraeth_http::IncomingRequest::from_hyper(request).await {
        Ok(incoming_request) => incoming_request,
        Err(error) => {
            let total_elapsed = started_at.elapsed();
            eprintln!(
                "[request {request_id}] parse_err total_ms={} error={:?}",
                total_elapsed.as_millis(),
                error,
            );

            let builder = hyper::Response::builder().status(400);
            return Ok(builder
                .body(Full::from("Bad Request: failed to read request body"))
                .unwrap());
        }
    };

    let method = incoming_request.method.clone();
    let host = incoming_request.host.clone();
    let path = incoming_request.path.clone();
    let query = incoming_request.query.clone();
    let body_bytes = incoming_request.body.len();
    let target = incoming_request.headers.get("x-amz-target").cloned();

    let outcome = app.handle_request(incoming_request).await;
    let total_elapsed = started_at.elapsed();

    match outcome {
        AppRequestOutcome {
            response: Ok(response),
            trace,
        } => {
            eprintln!(
                "[request {request_id}] method={} host={} path={} query={} target={} service={} region={} account={} principal={} access_key={} request_bytes={} response_bytes={} status={} auth_ms={} route_ms={} total_ms={}",
                method,
                host,
                path,
                query.as_deref().unwrap_or(""),
                target.as_deref().unwrap_or(""),
                trace.service.as_deref().unwrap_or(""),
                trace.region.as_deref().unwrap_or(""),
                trace.account_id.as_deref().unwrap_or(""),
                trace.principal.as_deref().unwrap_or(""),
                trace.access_key.as_deref().unwrap_or(""),
                body_bytes,
                response.body.len(),
                response.status_code,
                trace.auth_ms,
                trace.route_ms.unwrap_or(0),
                total_elapsed.as_millis(),
            );
            let mut builder = hyper::Response::builder().status(response.status_code);
            for (name, value) in response.headers {
                builder = builder.header(name, value);
            }
            Ok(builder.body(Full::from(response.body)).unwrap())
        }
        AppRequestOutcome {
            response: Err(e),
            trace,
        } => {
            let error_message = e.message();
            eprintln!(
                "[request {request_id}] method={} host={} path={} query={} target={} service={} region={} account={} principal={} access_key={} request_bytes={} response_bytes={} status={} auth_ms={} route_ms={} total_ms={} error={:?}",
                method,
                host,
                path,
                query.as_deref().unwrap_or(""),
                target.as_deref().unwrap_or(""),
                trace.service.as_deref().unwrap_or(""),
                trace.region.as_deref().unwrap_or(""),
                trace.account_id.as_deref().unwrap_or(""),
                trace.principal.as_deref().unwrap_or(""),
                trace.access_key.as_deref().unwrap_or(""),
                body_bytes,
                error_message.len(),
                e.status_code(),
                trace.auth_ms,
                trace.route_ms.unwrap_or(0),
                total_elapsed.as_millis(),
                e,
            );
            let builder = hyper::Response::builder().status(e.status_code());
            Ok(builder.body(Full::from(error_message)).unwrap())
        }
    }
}
