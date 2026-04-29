use std::{convert::Infallible, net::SocketAddr, sync::Arc, time::Instant};

use hiraeth_core::{
    ServiceResponse,
    tracing::{CompletedRequestTrace, TraceContext, TraceHttpRequest, TraceHttpResponse},
};
use hiraeth_http::IncomingRequest;
use http_body_util::Full;
use hyper::{
    Request,
    body::{Bytes, Incoming},
    server::conn::http1,
    service::service_fn,
};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use uuid::Uuid;

use crate::{app::App, request::AppRequestOutcome};

const AWS_REQUEST_ID_HEADER: &str = "x-amzn-requestid";

pub async fn serve(addr: SocketAddr, app: Arc<App>) -> anyhow::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    serve_listener(listener, app).await
}

pub async fn serve_listener(listener: TcpListener, app: Arc<App>) -> anyhow::Result<()> {
    loop {
        let (stream, peer_addr) = listener.accept().await?;
        let app = Arc::clone(&app);

        let service = service_fn(move |request| handle_request(Arc::clone(&app), request));
        tokio::spawn(async move {
            let io = TokioIo::new(stream);
            if let Err(e) = http1::Builder::new().serve_connection(io, service).await {
                tracing::error!(
                    peer_addr = %peer_addr,
                    error = ?e,
                    "connection error"
                );
            }
        });
    }
}

async fn handle_request(
    app: Arc<App>,
    request: Request<Incoming>,
) -> Result<hyper::Response<Full<Bytes>>, Infallible> {
    let request_id = Uuid::new_v4().to_string();
    let trace_context = TraceContext::new(request_id.clone());
    let started_at = Instant::now();
    let trace_started_at = chrono::Utc::now();

    let incoming_request = match IncomingRequest::from_hyper(request).await {
        Ok(incoming_request) => incoming_request,
        Err(error) => {
            let total_elapsed = started_at.elapsed();

            tracing::warn!(
                request_id = %request_id,
                total_ms = total_elapsed.as_millis() as u64,
                error = ?error,
                "failed to parse request"
            );

            let builder = hyper::Response::builder()
                .status(400)
                .header(AWS_REQUEST_ID_HEADER, request_id);
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
    let trace_request = TraceHttpRequest {
        method: method.clone(),
        host: host.clone(),
        path: path.clone(),
        query: query.clone(),
        headers: incoming_request.headers.clone(),
        body: incoming_request.body.clone(),
    };

    let outcome = app.handle_request(&trace_context, incoming_request).await;
    let total_elapsed = started_at.elapsed();

    match outcome {
        AppRequestOutcome {
            response: Ok(response),
            trace,
        } => {
            let response = with_aws_request_id(response, &request_id);
            tracing::info!(
                request_id = %request_id,
                method = %method,
                host = %host,
                path = %path,
                query = query.as_deref().unwrap_or(""),
                target = target.as_deref().unwrap_or(""),
                service = trace.service.as_deref().unwrap_or(""),
                region = trace.region.as_deref().unwrap_or(""),
                account = trace.account_id.as_deref().unwrap_or(""),
                principal = trace.principal.as_deref().unwrap_or(""),
                access_key = trace.access_key.as_deref().unwrap_or(""),
                request_bytes = body_bytes as u64,
                response_bytes = response.body.len() as u64,
                status = response.status_code,
                auth_ms = trace.auth_ms as u64,
                route_ms = trace.route_ms.unwrap_or(0) as u64,
                total_ms = total_elapsed.as_millis() as u64,
                "request handled"
            );
            app.record_trace(CompletedRequestTrace {
                request_id: request_id.clone(),
                started_at: trace_started_at,
                completed_at: chrono::Utc::now(),
                duration_ms: total_elapsed.as_millis(),
                auth_ms: trace.auth_ms,
                route_ms: trace.route_ms,
                service: trace.service,
                region: trace.region,
                account_id: trace.account_id,
                principal: trace.principal,
                access_key: trace.access_key,
                request: trace_request,
                response: TraceHttpResponse {
                    status_code: response.status_code,
                    headers: response.headers.clone(),
                    body: response.body.clone(),
                },
                error_message: None,
            })
            .await;
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
            let error_body = error_message.as_bytes().to_vec();

            tracing::warn!(
                request_id = %request_id,
                method = %method,
                host = %host,
                path = %path,
                query = query.as_deref().unwrap_or(""),
                target = target.as_deref().unwrap_or(""),
                service = trace.service.as_deref().unwrap_or(""),
                region = trace.region.as_deref().unwrap_or(""),
                account = trace.account_id.as_deref().unwrap_or(""),
                principal = trace.principal.as_deref().unwrap_or(""),
                access_key = trace.access_key.as_deref().unwrap_or(""),
                request_bytes = body_bytes as u64,
                response_bytes = error_body.len() as u64,
                status = e.status_code(),
                auth_ms = trace.auth_ms as u64,
                route_ms = trace.route_ms.unwrap_or(0) as u64,
                total_ms = total_elapsed.as_millis() as u64,
                error = ?e,
                "request failed"
            );
            app.record_trace(CompletedRequestTrace {
                request_id: request_id.clone(),
                started_at: trace_started_at,
                completed_at: chrono::Utc::now(),
                duration_ms: total_elapsed.as_millis(),
                auth_ms: trace.auth_ms,
                route_ms: trace.route_ms,
                service: trace.service,
                region: trace.region,
                account_id: trace.account_id,
                principal: trace.principal,
                access_key: trace.access_key,
                request: trace_request,
                response: TraceHttpResponse {
                    status_code: e.status_code(),
                    headers: vec![(AWS_REQUEST_ID_HEADER.to_string(), request_id.clone())],
                    body: error_body.clone(),
                },
                error_message: Some(error_message),
            })
            .await;
            let builder = hyper::Response::builder()
                .status(e.status_code())
                .header(AWS_REQUEST_ID_HEADER, request_id);
            Ok(builder.body(Full::from(error_body)).unwrap())
        }
    }
}

fn with_aws_request_id(mut response: ServiceResponse, request_id: &str) -> ServiceResponse {
    if !response
        .headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case(AWS_REQUEST_ID_HEADER))
    {
        response
            .headers
            .push((AWS_REQUEST_ID_HEADER.to_string(), request_id.to_string()));
    }

    if let Ok(body) = std::str::from_utf8(&response.body) {
        response.body = replace_xml_request_ids(body, request_id).into_bytes();
    }
    response
}

fn replace_xml_request_ids(body: &str, request_id: &str) -> String {
    let mut output = String::with_capacity(body.len());
    let mut remaining = body;
    let open = "<RequestId>";
    let close = "</RequestId>";

    while let Some(start) = remaining.find(open) {
        let (before, after_start) = remaining.split_at(start);
        output.push_str(before);
        output.push_str(open);
        let after_open = &after_start[open.len()..];

        let Some(end) = after_open.find(close) else {
            output.push_str(after_open);
            return output;
        };

        output.push_str(request_id);
        output.push_str(close);
        remaining = &after_open[end + close.len()..];
    }

    output.push_str(remaining);
    output
}
