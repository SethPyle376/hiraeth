use std::collections::HashMap;

use askama::Template;
use axum::{
    Router,
    extract::{Path, State},
    response::{Html, Redirect},
    routing::{get, post},
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
    pub(crate) detail_id: String,
    pub(crate) graph_id: String,
    pub(crate) span_id: String,
    pub(crate) parent_span_id: String,
    pub(crate) has_parent_span_id: bool,
    pub(crate) name: String,
    pub(crate) layer: String,
    pub(crate) status: String,
    pub(crate) status_class: &'static str,
    pub(crate) duration_ms: String,
    pub(crate) attributes: String,
    pub(crate) has_attributes: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TraceGraphView {
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) nodes: Vec<TraceGraphNodeView>,
    pub(crate) edges: Vec<TraceGraphEdgeView>,
}

#[derive(Debug, Clone)]
pub(crate) struct TraceGraphNodeView {
    pub(crate) graph_id: String,
    pub(crate) detail_id: String,
    pub(crate) parent_graph_id: String,
    pub(crate) has_parent_graph_id: bool,
    pub(crate) label: String,
    pub(crate) meta: String,
    pub(crate) status: String,
    pub(crate) status_class: &'static str,
    pub(crate) node_class: &'static str,
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) is_span: bool,
    pub(crate) is_tiny: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct TraceGraphEdgeView {
    pub(crate) parent_graph_id: String,
    pub(crate) child_graph_id: String,
    pub(crate) x1: i32,
    pub(crate) y1: i32,
    pub(crate) y_mid: i32,
    pub(crate) x2: i32,
    pub(crate) y2: i32,
    pub(crate) arrow_left_x: i32,
    pub(crate) arrow_right_x: i32,
    pub(crate) arrow_base_y: i32,
}

pub(crate) fn router() -> Router<WebState> {
    Router::new()
        .route("/", get(list_traces))
        .route("/clear", post(clear_traces))
        .route("/{request_id}", get(trace_detail))
}

async fn list_traces(State(state): State<WebState>) -> Result<Html<String>, WebError> {
    let traces = state.trace_store.list_request_traces(100).await?;
    let views = traces
        .into_iter()
        .map(trace_summary_view)
        .collect::<Vec<_>>();

    let stats_html = StatBlockGrid {
        grid_class: "grid-cols-1 sm:grid-cols-2",
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
        ],
    }
    .render()?;

    let page_header_html = PageHeader {
        eyebrow: "Local Traces".to_string(),
        title: "Request traces".to_string(),
        description: "Review recent AWS endpoint requests, stored payloads, and lifecycle spans."
            .to_string(),
        actions: vec![HeaderAction::link("Reload", "/traces", "btn btn-outline")],
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

async fn clear_traces(State(state): State<WebState>) -> Result<Redirect, WebError> {
    state.trace_store.clear_traces().await?;
    Ok(Redirect::to("/traces"))
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
    let span_views = spans
        .into_iter()
        .enumerate()
        .map(|(index, span)| trace_span_view(index, span))
        .collect::<Vec<_>>();
    let graph = trace_graph_view(&detail, &span_views);

    let page_header_html = PageHeader {
        eyebrow: "Request Trace".to_string(),
        title: format!("request {}", detail.request_id),
        description: format!(
            "{} {} returned {} in {} ms",
            detail.method, detail.path, detail.response_status_code, detail.duration_ms
        ),
        actions: vec![
            HeaderAction::link("Back", "/traces", "btn btn-ghost"),
            HeaderAction::link("Reload", format!("/traces/{request_id}"), "btn btn-outline"),
        ],
    }
    .render()?;

    Ok(Html(
        TraceDetailTemplate {
            page_header_html: &page_header_html,
            trace: &detail,
            graph: &graph,
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

fn trace_span_view(index: usize, span: StoredTraceSpan) -> TraceSpanView {
    let parent_span_id = span.parent_span_id.clone().unwrap_or_default();
    TraceSpanView {
        detail_id: format!("trace-span-detail-{index}"),
        graph_id: format!("trace-span-node-{index}"),
        span_id: span.span_id,
        has_parent_span_id: !parent_span_id.is_empty(),
        parent_span_id,
        name: span.name,
        layer: span.layer,
        status_class: span_status_class(&span.status),
        status: span.status,
        duration_ms: span.duration_ms.to_string(),
        attributes: pretty_json(&span.attributes),
        has_attributes: !span.attributes.is_empty(),
    }
}

fn trace_graph_view(trace: &TraceDetailView, spans: &[TraceSpanView]) -> TraceGraphView {
    if spans.is_empty() {
        return TraceGraphView::default();
    }

    const NODE_WIDTH: i32 = 168;
    const NODE_HEIGHT: i32 = 52;
    const TINY_NODE_HEIGHT: i32 = 34;
    const X_GAP: i32 = 42;
    const Y_GAP: i32 = 54;
    const MARGIN: i32 = 24;

    let has_parent_links = spans.iter().any(|span| span.has_parent_span_id);
    let span_index_by_id = spans
        .iter()
        .enumerate()
        .map(|(index, span)| (span.span_id.as_str(), index))
        .collect::<HashMap<_, _>>();

    let depths = if has_parent_links {
        spans
            .iter()
            .enumerate()
            .map(|(index, _)| span_depth(index, spans, &span_index_by_id))
            .collect::<Vec<_>>()
    } else {
        (0..spans.len()).map(|index| index + 1).collect::<Vec<_>>()
    };

    let mut row_counts_by_depth = HashMap::<usize, usize>::new();
    let mut rows = Vec::with_capacity(spans.len());
    for depth in &depths {
        let row = row_counts_by_depth.entry(*depth).or_default();
        rows.push(*row);
        *row += 1;
    }

    let max_row = rows.iter().copied().max().unwrap_or_default() as i32;
    let root_x = MARGIN + ((max_row * (NODE_WIDTH + X_GAP)) / 2);
    let root = TraceGraphNodeView {
        graph_id: "trace-root-node".to_string(),
        detail_id: String::new(),
        parent_graph_id: String::new(),
        has_parent_graph_id: false,
        label: format!("{} {}", trace.method, trace.path),
        meta: format!("{} ms total", trace.duration_ms),
        status: trace.response_status_code.to_string(),
        status_class: trace.response_status_class,
        node_class: graph_root_node_class_for_http_status(trace.response_status_code),
        x: root_x,
        y: MARGIN,
        width: NODE_WIDTH,
        height: NODE_HEIGHT,
        is_span: false,
        is_tiny: false,
    };

    let mut nodes = vec![root];
    let mut edges = Vec::new();

    for (index, span) in spans.iter().enumerate() {
        let depth = depths[index] as i32;
        let row = rows[index] as i32;
        let x = MARGIN + row * (NODE_WIDTH + X_GAP);
        let y = MARGIN + depth * (NODE_HEIGHT + Y_GAP);

        let parent_node = if has_parent_links {
            span_index_by_id
                .get(span.parent_span_id.as_str())
                .map(|parent_index| {
                    let parent_depth = depths[*parent_index] as i32;
                    let parent_row = rows[*parent_index] as i32;
                    let parent_height = if spans[*parent_index].duration_ms == "0" {
                        TINY_NODE_HEIGHT
                    } else {
                        NODE_HEIGHT
                    };
                    (
                        MARGIN + parent_row * (NODE_WIDTH + X_GAP),
                        MARGIN + parent_depth * (NODE_HEIGHT + Y_GAP),
                        parent_height,
                        spans[*parent_index].graph_id.clone(),
                    )
                })
        } else if index > 0 {
            let parent_depth = depths[index - 1] as i32;
            let parent_row = rows[index - 1] as i32;
            let parent_height = if spans[index - 1].duration_ms == "0" {
                TINY_NODE_HEIGHT
            } else {
                NODE_HEIGHT
            };
            Some((
                MARGIN + parent_row * (NODE_WIDTH + X_GAP),
                MARGIN + parent_depth * (NODE_HEIGHT + Y_GAP),
                parent_height,
                spans[index - 1].graph_id.clone(),
            ))
        } else {
            None
        };

        let parent_graph_id = parent_node
            .as_ref()
            .map(|(_, _, _, parent_graph_id)| parent_graph_id.clone())
            .unwrap_or_else(|| "trace-root-node".to_string());
        let is_tiny = span.duration_ms == "0";
        let node_height = if is_tiny {
            TINY_NODE_HEIGHT
        } else {
            NODE_HEIGHT
        };

        nodes.push(TraceGraphNodeView {
            graph_id: span.graph_id.clone(),
            detail_id: span.detail_id.clone(),
            parent_graph_id: parent_graph_id.clone(),
            has_parent_graph_id: true,
            label: span.name.clone(),
            meta: format!("{} / {} ms", span.layer, span.duration_ms),
            status: span.status.clone(),
            status_class: span.status_class,
            node_class: graph_node_class_for_span_status(&span.status, is_tiny),
            x,
            y,
            width: NODE_WIDTH,
            height: node_height,
            is_span: true,
            is_tiny,
        });

        let (parent_x, parent_y, parent_height) = parent_node
            .map(|(parent_x, parent_y, parent_height, _)| (parent_x, parent_y, parent_height))
            .unwrap_or((root_x, MARGIN, NODE_HEIGHT));
        let y1 = parent_y + parent_height;
        let y2 = y - 8;
        edges.push(TraceGraphEdgeView {
            parent_graph_id,
            child_graph_id: span.graph_id.clone(),
            x1: parent_x + NODE_WIDTH / 2,
            y1,
            y_mid: y1 + ((y2 - y1) / 2),
            x2: x + NODE_WIDTH / 2,
            y2,
            arrow_left_x: x + NODE_WIDTH / 2 - 5,
            arrow_right_x: x + NODE_WIDTH / 2 + 5,
            arrow_base_y: y2 - 8,
        });
    }

    let max_depth = depths.iter().copied().max().unwrap_or_default() as i32;
    TraceGraphView {
        width: MARGIN * 2 + (max_row + 1) * NODE_WIDTH + max_row * X_GAP,
        height: MARGIN * 2 + (max_depth + 1) * NODE_HEIGHT + max_depth * Y_GAP,
        nodes,
        edges,
    }
}

fn span_depth(
    index: usize,
    spans: &[TraceSpanView],
    span_index_by_id: &HashMap<&str, usize>,
) -> usize {
    let span = &spans[index];
    if !span.has_parent_span_id {
        return 1;
    }

    span_index_by_id
        .get(span.parent_span_id.as_str())
        .map(|parent_index| span_depth(*parent_index, spans, span_index_by_id) + 1)
        .unwrap_or(1)
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

fn graph_root_node_class_for_http_status(status_code: u16) -> &'static str {
    if status_code >= 500 {
        "border-error/80 bg-error/15 text-error-content shadow-[0_0_0_1px_rgba(0,0,0,0.2)]"
    } else if status_code >= 400 {
        "border-warning/80 bg-warning/15 text-warning-content shadow-[0_0_0_1px_rgba(0,0,0,0.2)]"
    } else {
        "border-primary/80 bg-primary/15 shadow-[0_0_0_1px_rgba(0,0,0,0.2)]"
    }
}

fn span_status_class(status: &str) -> &'static str {
    match status {
        "ok" | "allow" => "badge-success",
        "deny" | "error" => "badge-error",
        _ => "badge-outline",
    }
}

fn graph_node_class_for_span_status(status: &str, is_tiny: bool) -> &'static str {
    match (status, is_tiny) {
        ("ok" | "allow", true) => {
            "border-success/40 bg-success/5 opacity-80 hover:border-success hover:bg-success/10"
        }
        ("ok" | "allow", false) => {
            "border-success/60 bg-success/10 hover:border-success hover:bg-success/15"
        }
        ("deny" | "error", true) => {
            "border-error/60 bg-error/10 opacity-90 hover:border-error hover:bg-error/15"
        }
        ("deny" | "error", false) => {
            "border-error/70 bg-error/10 hover:border-error hover:bg-error/15"
        }
        (_, true) => "border-base-300/70 bg-base-100/70 hover:border-primary hover:bg-base-200",
        (_, false) => "border-base-300 bg-base-100 hover:border-primary hover:bg-base-200",
    }
}
