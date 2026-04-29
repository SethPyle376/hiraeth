use std::{collections::HashMap, fmt::Display, time::Instant};

use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceHttpRequest {
    pub method: String,
    pub host: String,
    pub path: String,
    pub query: Option<String>,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceHttpResponse {
    pub status_code: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletedRequestTrace {
    pub request_id: String,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub duration_ms: u128,
    pub auth_ms: u128,
    pub route_ms: Option<u128>,
    pub service: Option<String>,
    pub region: Option<String>,
    pub account_id: Option<String>,
    pub principal: Option<String>,
    pub access_key: Option<String>,
    pub request: TraceHttpRequest,
    pub response: TraceHttpResponse,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestTraceSummary {
    pub request_id: String,
    pub started_at: DateTime<Utc>,
    pub duration_ms: u128,
    pub service: Option<String>,
    pub action: Option<String>,
    pub region: Option<String>,
    pub account_id: Option<String>,
    pub principal: Option<String>,
    pub method: String,
    pub path: String,
    pub response_status_code: u16,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TraceRequestFilters {
    pub service: Option<String>,
    pub action: Option<String>,
    pub status: Option<TraceRequestStatusFilter>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceRequestStatusFilter {
    Ok,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRequestTrace {
    pub request_id: String,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub duration_ms: u128,
    pub auth_ms: u128,
    pub route_ms: Option<u128>,
    pub service: Option<String>,
    pub region: Option<String>,
    pub account_id: Option<String>,
    pub principal: Option<String>,
    pub access_key: Option<String>,
    pub request: TraceHttpRequest,
    pub response: TraceHttpResponse,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredTraceSpan {
    pub request_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub name: String,
    pub layer: String,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub duration_ms: u128,
    pub status: String,
    pub attributes: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceSpanRecord {
    pub request_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub name: String,
    pub layer: String,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub duration_ms: u128,
    pub status: String,
    pub attributes: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceContext {
    pub request_id: String,
    parent_span_id: Option<String>,
}

impl TraceContext {
    pub fn new(request_id: impl Into<String>) -> Self {
        Self {
            request_id: request_id.into(),
            parent_span_id: None,
        }
    }

    pub fn start_span(&self) -> TraceSpanTimer {
        TraceSpanTimer {
            span_id: new_span_id(&self.request_id),
            parent_span_id: self.parent_span_id.clone(),
            started_at: Utc::now(),
            started_instant: Instant::now(),
        }
    }

    pub fn child_context(&self, timer: &TraceSpanTimer) -> Self {
        Self {
            request_id: self.request_id.clone(),
            parent_span_id: Some(timer.span_id.clone()),
        }
    }

    pub async fn record_span<R>(
        &self,
        recorder: &R,
        timer: TraceSpanTimer,
        name: impl Into<String>,
        layer: impl Into<String>,
        status: impl Into<String>,
        attributes: HashMap<String, String>,
    ) -> Result<(), TraceRecordError>
    where
        R: TraceRecorder + ?Sized,
    {
        recorder
            .record_span(TraceSpanRecord {
                request_id: self.request_id.clone(),
                span_id: timer.span_id,
                parent_span_id: timer.parent_span_id,
                name: name.into(),
                layer: layer.into(),
                started_at: timer.started_at,
                completed_at: Utc::now(),
                duration_ms: timer.started_instant.elapsed().as_millis(),
                status: status.into(),
                attributes,
            })
            .await
    }

    pub async fn record_span_or_warn<R>(
        &self,
        recorder: &R,
        timer: TraceSpanTimer,
        name: &'static str,
        layer: &'static str,
        status: &'static str,
        attributes: HashMap<String, String>,
    ) where
        R: TraceRecorder + ?Sized,
    {
        if let Err(error) = self
            .record_span(recorder, timer, name, layer, status, attributes)
            .await
        {
            tracing::warn!(
                request_id = %self.request_id,
                span = name,
                error = %error,
                "failed to record trace span"
            );
        }
    }
}

fn new_span_id(request_id: &str) -> String {
    format!(
        "{}-{}",
        request_id,
        Utc::now().timestamp_nanos_opt().unwrap_or_default()
    )
}

#[derive(Debug, Clone)]
pub struct TraceSpanTimer {
    span_id: String,
    parent_span_id: Option<String>,
    started_at: DateTime<Utc>,
    started_instant: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TraceRecordError {
    StorageFailure(String),
}

impl Display for TraceRecordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TraceRecordError::StorageFailure(msg) => {
                write!(f, "trace storage failure: {}", msg)
            }
        }
    }
}

impl std::error::Error for TraceRecordError {}

#[async_trait]
pub trait TraceRecorder: Sync {
    async fn record_request_trace(
        &self,
        trace: CompletedRequestTrace,
    ) -> Result<(), TraceRecordError>;

    async fn record_span(&self, span: TraceSpanRecord) -> Result<(), TraceRecordError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoopTraceRecorder;

#[async_trait]
impl TraceRecorder for NoopTraceRecorder {
    async fn record_request_trace(
        &self,
        _trace: CompletedRequestTrace,
    ) -> Result<(), TraceRecordError> {
        Ok(())
    }

    async fn record_span(&self, _span: TraceSpanRecord) -> Result<(), TraceRecordError> {
        Ok(())
    }
}
