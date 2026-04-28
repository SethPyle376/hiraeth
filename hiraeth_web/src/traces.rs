use askama::Template;
use axum::{
    Router,
    extract::{Path, State},
    response::Html,
    routing::get,
};
use hiraeth_core::tracing::{RequestTraceSummary, StoredRequestTrace, StoredTraceSpan};

use crate::{
    WebState,
    components::{HeaderAction, PageHeader, StatBlock, StatBlockGrid},
    error::WebError,
    templates::{TraceDetailTemplate, TraceListTemplate},
};

#[derive(Debug, Clone)]
pub(crate) struct TraceSummaryView {
    pub(crate) request_id: String,
    pub(crate) started_at: String,
    pub(crate) duration_ms: String,
    pub(crate) service: String,
    pub(crate) region: String,
    pub(crate) account_id: String,
    pub(crate) principal: String,
    pub(crate) method: String,
    pub(crate) path: String,
    pub(crate) status_code: u16,
    pub(crate) status_class: &'static str,
    pub(crate) error_message: String,
    pub(crate) has_error: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct TraceDetailView {
    pub(crate) request_id: String,
    pub(crate) started_at: String,
    pub(crate) completed_at: String,
    pub(crate) duration_ms: String,
    pub(crate) auth_ms: String,
    pub(crate) route_ms: String,
    pub(crate) service: String,
    pub(crate) region: String,
    pub(crate) account_id: String,
    pub(crate) principal: String,
    pub(crate) access_key: String,
    pub(crate) method: String,
    pub(crate) host: String,
    pub(crate) path: String,
    pub(crate) query: String,
    pub(crate) request_headers: String,
    pub(crate) request_body: String,
    pub(crate) response_status_code: u16,
    pub(crate) response_status_class: &'static str,
    pub(crate) response_headers: String,
    pub(crate) response_body: String,
    pub(crate) error_message: String,
    pub(crate) has_error: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct TraceSpanView {
    pub(crate) name: String,
    pub(crate) layer: String,
    pub(crate) status: String,
    pub(crate) status_class: &'static str,
    pub(crate) duration_ms: String,
    pub(crate) attributes: String,
    pub(crate) has_attributes: bool,
}

pub(crate) fn router() -> Router<WebState> {
    Router::new()
        .route("/", get(list_traces))
        .route("/{request_id}", get(trace_detail))
}

async fn list_traces(State(state): State<WebState>) -> Result<Html<String>, WebError> {
    let traces = state.trace_store.list_request_traces(100).await?;
    let views = traces
        .into_iter()
        .map(trace_summary_view)
        .collect::<Vec<_>>();

    let stats_html = StatBlockGrid {
        grid_class: "grid-cols-1 sm:grid-cols-3",
        blocks: vec![
            StatBlock {
                title: "Requests".to_string(),
                value: views.len().to_string(),
                value_class: "text-primary",
                description: "recent traces retained locally".to_string(),
            },
            StatBlock {
                title: "Failures".to_string(),
                value: views
                    .iter()
                    .filter(|trace| trace.status_code >= 400)
                    .count()
                    .to_string(),
                value_class: "text-error",
                description: "responses with error status".to_string(),
            },
            StatBlock {
                title: "Window".to_string(),
                value: "100".to_string(),
                value_class: "text-accent",
                description: "most recent requests shown".to_string(),
            },
        ],
    }
    .render()?;

    let page_header_html = PageHeader {
        eyebrow: "Local Traces".to_string(),
        title: "Request traces".to_string(),
        description: "Review recent AWS endpoint requests, stored payloads, and lifecycle spans."
            .to_string(),
        actions: vec![HeaderAction::link("Reload", "/traces", "btn-outline")],
    }
    .render()?;

    Ok(Html(
        TraceListTemplate {
            page_header_html: &page_header_html,
            stats_html: &stats_html,
            traces: &views,
            has_traces: !views.is_empty(),
        }
        .render()?,
    ))
}

async fn trace_detail(
    State(state): State<WebState>,
    Path(request_id): Path<String>,
) -> Result<Html<String>, WebError> {
    let trace = state
        .trace_store
        .get_request_trace(&request_id)
        .await?
        .ok_or_else(|| WebError::bad_request(format!("Trace {request_id} was not found")))?;
    let spans = state.trace_store.list_trace_spans(&request_id).await?;

    let detail = trace_detail_view(trace);
    let span_views = spans.into_iter().map(trace_span_view).collect::<Vec<_>>();

    let page_header_html = PageHeader {
        eyebrow: "Request Trace".to_string(),
        title: format!("request {}", detail.request_id),
        description: format!(
            "{} {} returned {} in {} ms",
            detail.method, detail.path, detail.response_status_code, detail.duration_ms
        ),
        actions: vec![
            HeaderAction::link("Back", "/traces", "btn-ghost"),
            HeaderAction::link("Reload", format!("/traces/{request_id}"), "btn-outline"),
        ],
    }
    .render()?;

    Ok(Html(
        TraceDetailTemplate {
            page_header_html: &page_header_html,
            trace: &detail,
            spans: &span_views,
            has_spans: !span_views.is_empty(),
        }
        .render()?,
    ))
}

fn trace_summary_view(trace: RequestTraceSummary) -> TraceSummaryView {
    TraceSummaryView {
        request_id: trace.request_id,
        started_at: trace.started_at.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
        duration_ms: trace.duration_ms.to_string(),
        service: trace.service.unwrap_or_else(|| "-".to_string()),
        region: trace.region.unwrap_or_else(|| "-".to_string()),
        account_id: trace.account_id.unwrap_or_else(|| "-".to_string()),
        principal: trace.principal.unwrap_or_else(|| "-".to_string()),
        method: trace.method,
        path: trace.path,
        status_code: trace.response_status_code,
        status_class: status_class(trace.response_status_code),
        error_message: trace.error_message.clone().unwrap_or_default(),
        has_error: trace.error_message.is_some(),
    }
}

fn trace_detail_view(trace: StoredRequestTrace) -> TraceDetailView {
    let response_status_code = trace.response.status_code;
    TraceDetailView {
        request_id: trace.request_id,
        started_at: trace.started_at.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
        completed_at: trace
            .completed_at
            .format("%Y-%m-%d %H:%M:%S UTC")
            .to_string(),
        duration_ms: trace.duration_ms.to_string(),
        auth_ms: trace.auth_ms.to_string(),
        route_ms: trace
            .route_ms
            .map(|route_ms| route_ms.to_string())
            .unwrap_or_else(|| "-".to_string()),
        service: trace.service.unwrap_or_else(|| "-".to_string()),
        region: trace.region.unwrap_or_else(|| "-".to_string()),
        account_id: trace.account_id.unwrap_or_else(|| "-".to_string()),
        principal: trace.principal.unwrap_or_else(|| "-".to_string()),
        access_key: trace.access_key.unwrap_or_else(|| "-".to_string()),
        method: trace.request.method,
        host: trace.request.host,
        path: trace.request.path,
        query: trace.request.query.unwrap_or_default(),
        request_headers: pretty_json(&trace.request.headers),
        request_body: body_text(&trace.request.body),
        response_status_code,
        response_status_class: status_class(response_status_code),
        response_headers: pretty_json(&trace.response.headers),
        response_body: body_text(&trace.response.body),
        error_message: trace.error_message.clone().unwrap_or_default(),
        has_error: trace.error_message.is_some(),
    }
}

fn trace_span_view(span: StoredTraceSpan) -> TraceSpanView {
    TraceSpanView {
        name: span.name,
        layer: span.layer,
        status_class: span_status_class(&span.status),
        status: span.status,
        duration_ms: span.duration_ms.to_string(),
        attributes: pretty_json(&span.attributes),
        has_attributes: !span.attributes.is_empty(),
    }
}

fn pretty_json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string())
}

fn body_text(body: &[u8]) -> String {
    String::from_utf8_lossy(body).to_string()
}

fn status_class(status_code: u16) -> &'static str {
    if status_code >= 500 {
        "badge-error"
    } else if status_code >= 400 {
        "badge-warning"
    } else {
        "badge-success"
    }
}

fn span_status_class(status: &str) -> &'static str {
    match status {
        "ok" | "allow" => "badge-success",
        "deny" | "error" => "badge-error",
        _ => "badge-outline",
    }
}
