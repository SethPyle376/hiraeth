use async_trait::async_trait;
use std::collections::HashMap;

use chrono::{DateTime, Utc};
use hiraeth_core::tracing::{
    CompletedRequestTrace, RequestTraceSummary, StoredRequestTrace, StoredTraceSpan,
    TraceHttpRequest, TraceHttpResponse, TraceRecordError, TraceRecorder, TraceSpanRecord,
};
use sqlx::Row;

#[derive(Clone)]
pub struct SqliteTraceStore {
    pool: sqlx::SqlitePool,
}

impl SqliteTraceStore {
    pub fn new(pool: &sqlx::SqlitePool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn list_request_traces(
        &self,
        limit: i64,
    ) -> Result<Vec<RequestTraceSummary>, TraceRecordError> {
        let rows = sqlx::query(
            r#"
            SELECT
                request_id,
                started_at,
                duration_ms,
                service,
                region,
                account_id,
                principal,
                method,
                path,
                response_status_code,
                error_message
            FROM hiraeth_trace_request
            ORDER BY started_at DESC, request_id DESC
            LIMIT ?
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_trace_error)?;

        rows.into_iter()
            .map(|row| {
                Ok(RequestTraceSummary {
                    request_id: row.get("request_id"),
                    started_at: parse_datetime(row.get::<String, _>("started_at"))?,
                    duration_ms: row.get::<i64, _>("duration_ms") as u128,
                    service: row.get("service"),
                    region: row.get("region"),
                    account_id: row.get("account_id"),
                    principal: row.get("principal"),
                    method: row.get("method"),
                    path: row.get("path"),
                    response_status_code: row.get::<i64, _>("response_status_code") as u16,
                    error_message: row.get("error_message"),
                })
            })
            .collect()
    }

    pub async fn get_request_trace(
        &self,
        request_id: &str,
    ) -> Result<Option<StoredRequestTrace>, TraceRecordError> {
        let row = sqlx::query(
            r#"
            SELECT
                request_id,
                started_at,
                completed_at,
                duration_ms,
                auth_ms,
                route_ms,
                service,
                region,
                account_id,
                principal,
                access_key,
                method,
                host,
                path,
                query,
                request_headers_json,
                request_body,
                response_status_code,
                response_headers_json,
                response_body,
                error_message
            FROM hiraeth_trace_request
            WHERE request_id = ?
            "#,
        )
        .bind(request_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_trace_error)?;

        row.map(|row| {
            Ok(StoredRequestTrace {
                request_id: row.get("request_id"),
                started_at: parse_datetime(row.get::<String, _>("started_at"))?,
                completed_at: parse_datetime(row.get::<String, _>("completed_at"))?,
                duration_ms: row.get::<i64, _>("duration_ms") as u128,
                auth_ms: row.get::<i64, _>("auth_ms") as u128,
                route_ms: row
                    .get::<Option<i64>, _>("route_ms")
                    .map(|value| value as u128),
                service: row.get("service"),
                region: row.get("region"),
                account_id: row.get("account_id"),
                principal: row.get("principal"),
                access_key: row.get("access_key"),
                request: TraceHttpRequest {
                    method: row.get("method"),
                    host: row.get("host"),
                    path: row.get("path"),
                    query: row.get("query"),
                    headers: parse_json(row.get::<String, _>("request_headers_json"))?,
                    body: row.get("request_body"),
                },
                response: TraceHttpResponse {
                    status_code: row.get::<i64, _>("response_status_code") as u16,
                    headers: parse_json(row.get::<String, _>("response_headers_json"))?,
                    body: row.get("response_body"),
                },
                error_message: row.get("error_message"),
            })
        })
        .transpose()
    }

    pub async fn list_trace_spans(
        &self,
        request_id: &str,
    ) -> Result<Vec<StoredTraceSpan>, TraceRecordError> {
        let rows = sqlx::query(
            r#"
            SELECT
                request_id,
                span_id,
                parent_span_id,
                name,
                layer,
                started_at,
                completed_at,
                duration_ms,
                status,
                attributes_json
            FROM hiraeth_trace_span
            WHERE request_id = ?
            ORDER BY started_at ASC, id ASC
            "#,
        )
        .bind(request_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_trace_error)?;

        rows.into_iter()
            .map(|row| {
                Ok(StoredTraceSpan {
                    request_id: row.get("request_id"),
                    span_id: row.get("span_id"),
                    parent_span_id: row.get("parent_span_id"),
                    name: row.get("name"),
                    layer: row.get("layer"),
                    started_at: parse_datetime(row.get::<String, _>("started_at"))?,
                    completed_at: parse_datetime(row.get::<String, _>("completed_at"))?,
                    duration_ms: row.get::<i64, _>("duration_ms") as u128,
                    status: row.get("status"),
                    attributes: parse_json(row.get::<String, _>("attributes_json"))?,
                })
            })
            .collect()
    }

    pub async fn clear_traces(&self) -> Result<(), TraceRecordError> {
        let mut transaction = self.pool.begin().await.map_err(map_trace_error)?;

        sqlx::query("DELETE FROM hiraeth_trace_span")
            .execute(&mut *transaction)
            .await
            .map_err(map_trace_error)?;

        sqlx::query("DELETE FROM hiraeth_trace_request")
            .execute(&mut *transaction)
            .await
            .map_err(map_trace_error)?;

        transaction.commit().await.map_err(map_trace_error)
    }
}

fn parse_datetime(value: String) -> Result<DateTime<Utc>, TraceRecordError> {
    DateTime::parse_from_rfc3339(&value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|err| TraceRecordError::StorageFailure(err.to_string()))
}

fn parse_json<T: serde::de::DeserializeOwned>(value: String) -> Result<T, TraceRecordError> {
    serde_json::from_str(&value).map_err(|err| TraceRecordError::StorageFailure(err.to_string()))
}

fn map_trace_error(error: sqlx::Error) -> TraceRecordError {
    TraceRecordError::StorageFailure(error.to_string())
}

#[async_trait]
impl TraceRecorder for SqliteTraceStore {
    async fn record_request_trace(
        &self,
        trace: CompletedRequestTrace,
    ) -> Result<(), TraceRecordError> {
        let request_headers_json = serde_json::to_string(&trace.request.headers)
            .map_err(|err| TraceRecordError::StorageFailure(err.to_string()))?;
        let response_headers_json = serde_json::to_string(&trace.response.headers)
            .map_err(|err| TraceRecordError::StorageFailure(err.to_string()))?;

        sqlx::query(
            r#"
            INSERT INTO hiraeth_trace_request (
                request_id,
                started_at,
                completed_at,
                duration_ms,
                auth_ms,
                route_ms,
                service,
                region,
                account_id,
                principal,
                access_key,
                method,
                host,
                path,
                query,
                request_headers_json,
                request_body,
                response_status_code,
                response_headers_json,
                response_body,
                error_message
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(trace.request_id)
        .bind(trace.started_at)
        .bind(trace.completed_at)
        .bind(trace.duration_ms as i64)
        .bind(trace.auth_ms as i64)
        .bind(trace.route_ms.map(|route_ms| route_ms as i64))
        .bind(trace.service)
        .bind(trace.region)
        .bind(trace.account_id)
        .bind(trace.principal)
        .bind(trace.access_key)
        .bind(trace.request.method)
        .bind(trace.request.host)
        .bind(trace.request.path)
        .bind(trace.request.query)
        .bind(request_headers_json)
        .bind(trace.request.body)
        .bind(trace.response.status_code as i64)
        .bind(response_headers_json)
        .bind(trace.response.body)
        .bind(trace.error_message)
        .execute(&self.pool)
        .await
        .map_err(|err| TraceRecordError::StorageFailure(err.to_string()))?;

        Ok(())
    }

    async fn record_span(&self, span: TraceSpanRecord) -> Result<(), TraceRecordError> {
        let attributes_json = serde_json::to_string(&span.attributes)
            .map_err(|err| TraceRecordError::StorageFailure(err.to_string()))?;

        sqlx::query(
            r#"
            INSERT INTO hiraeth_trace_span (
                request_id,
                span_id,
                parent_span_id,
                name,
                layer,
                started_at,
                completed_at,
                duration_ms,
                status,
                attributes_json
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(span.request_id)
        .bind(span.span_id)
        .bind(span.parent_span_id)
        .bind(span.name)
        .bind(span.layer)
        .bind(span.started_at)
        .bind(span.completed_at)
        .bind(span.duration_ms as i64)
        .bind(span.status)
        .bind(attributes_json)
        .execute(&self.pool)
        .await
        .map_err(|err| TraceRecordError::StorageFailure(err.to_string()))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::Utc;
    use hiraeth_core::tracing::{
        CompletedRequestTrace, TraceHttpRequest, TraceHttpResponse, TraceRecorder, TraceSpanRecord,
    };
    use sqlx::Row;
    use tempfile::TempDir;

    use crate::{get_store_pool, run_migrations};

    use super::SqliteTraceStore;

    async fn test_store() -> (TempDir, sqlx::SqlitePool, SqliteTraceStore) {
        let temp_dir = TempDir::new().expect("temp dir should be created");
        let db_path = temp_dir.path().join("trace.db");
        let db_url = format!("sqlite://{}", db_path.display());
        let pool = get_store_pool(&db_url)
            .await
            .expect("pool should be created");
        run_migrations(&pool).await.expect("migrations should run");

        let store = SqliteTraceStore::new(&pool);
        (temp_dir, pool, store)
    }

    #[tokio::test]
    async fn record_request_trace_persists_request_and_response_bodies() {
        let (_temp_dir, pool, store) = test_store().await;
        let mut request_headers = HashMap::new();
        request_headers.insert(
            "x-amz-target".to_string(),
            "AmazonSQS.SendMessage".to_string(),
        );

        store
            .record_request_trace(CompletedRequestTrace {
                request_id: "request-7".to_string(),
                started_at: Utc::now(),
                completed_at: Utc::now(),
                duration_ms: 12,
                auth_ms: 3,
                route_ms: Some(8),
                service: Some("sqs".to_string()),
                region: Some("us-east-1".to_string()),
                account_id: Some("000000000000".to_string()),
                principal: Some("test".to_string()),
                access_key: Some("test".to_string()),
                request: TraceHttpRequest {
                    method: "POST".to_string(),
                    host: "localhost:4566".to_string(),
                    path: "/".to_string(),
                    query: None,
                    headers: request_headers,
                    body: br#"{"QueueUrl":"http://localhost:4566/000000000000/orders"}"#.to_vec(),
                },
                response: TraceHttpResponse {
                    status_code: 200,
                    headers: vec![(
                        "content-type".to_string(),
                        "application/x-amz-json-1.0".to_string(),
                    )],
                    body: br#"{"MessageId":"abc"}"#.to_vec(),
                },
                error_message: None,
            })
            .await
            .expect("trace should be recorded");

        let row = sqlx::query(
            "SELECT request_id, service, request_body, response_status_code, response_body
             FROM hiraeth_trace_request",
        )
        .fetch_one(&pool)
        .await
        .expect("trace row should exist");

        assert_eq!(row.get::<String, _>("request_id"), "request-7");
        assert_eq!(row.get::<String, _>("service"), "sqs");
        assert_eq!(
            row.get::<Vec<u8>, _>("request_body"),
            br#"{"QueueUrl":"http://localhost:4566/000000000000/orders"}"#.to_vec()
        );
        assert_eq!(row.get::<i64, _>("response_status_code"), 200);
        assert_eq!(
            row.get::<Vec<u8>, _>("response_body"),
            br#"{"MessageId":"abc"}"#.to_vec()
        );

        let summaries = store
            .list_request_traces(10)
            .await
            .expect("trace summaries should load");
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].request_id, "request-7");
        assert_eq!(summaries[0].service.as_deref(), Some("sqs"));
        assert_eq!(summaries[0].response_status_code, 200);

        let stored = store
            .get_request_trace("request-7")
            .await
            .expect("trace should load")
            .expect("trace should exist");
        assert_eq!(
            stored.request.body,
            br#"{"QueueUrl":"http://localhost:4566/000000000000/orders"}"#.to_vec()
        );
        assert_eq!(stored.response.body, br#"{"MessageId":"abc"}"#.to_vec());
    }

    #[tokio::test]
    async fn record_span_persists_span_attributes() {
        let (_temp_dir, _pool, store) = test_store().await;
        let mut attributes = HashMap::new();
        attributes.insert("action".to_string(), "sqs:SendMessage".to_string());
        attributes.insert(
            "resource".to_string(),
            "arn:aws:sqs:us-east-1:000000000000:orders".to_string(),
        );

        store
            .record_span(TraceSpanRecord {
                request_id: "request-7".to_string(),
                span_id: "span-1".to_string(),
                parent_span_id: None,
                name: "authz.evaluate".to_string(),
                layer: "router".to_string(),
                started_at: Utc::now(),
                completed_at: Utc::now(),
                duration_ms: 2,
                status: "allow".to_string(),
                attributes,
            })
            .await
            .expect("span should be recorded");

        let spans = store
            .list_trace_spans("request-7")
            .await
            .expect("spans should load");

        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].name, "authz.evaluate");
        assert_eq!(spans[0].status, "allow");
        assert_eq!(
            spans[0].attributes.get("action").map(String::as_str),
            Some("sqs:SendMessage")
        );
    }

    #[tokio::test]
    async fn clear_traces_removes_requests_and_spans() {
        let (_temp_dir, _pool, store) = test_store().await;

        store
            .record_request_trace(CompletedRequestTrace {
                request_id: "request-7".to_string(),
                started_at: Utc::now(),
                completed_at: Utc::now(),
                duration_ms: 12,
                auth_ms: 3,
                route_ms: Some(8),
                service: Some("sqs".to_string()),
                region: Some("us-east-1".to_string()),
                account_id: Some("000000000000".to_string()),
                principal: Some("test".to_string()),
                access_key: Some("test".to_string()),
                request: TraceHttpRequest {
                    method: "POST".to_string(),
                    host: "localhost:4566".to_string(),
                    path: "/".to_string(),
                    query: None,
                    headers: HashMap::new(),
                    body: Vec::new(),
                },
                response: TraceHttpResponse {
                    status_code: 200,
                    headers: Vec::new(),
                    body: Vec::new(),
                },
                error_message: None,
            })
            .await
            .expect("trace should be recorded");

        store
            .record_span(TraceSpanRecord {
                request_id: "request-7".to_string(),
                span_id: "span-1".to_string(),
                parent_span_id: None,
                name: "action.handle".to_string(),
                layer: "router".to_string(),
                started_at: Utc::now(),
                completed_at: Utc::now(),
                duration_ms: 4,
                status: "ok".to_string(),
                attributes: HashMap::new(),
            })
            .await
            .expect("span should be recorded");

        store.clear_traces().await.expect("traces should clear");

        assert!(
            store
                .list_request_traces(10)
                .await
                .expect("request traces should load")
                .is_empty()
        );
        assert!(
            store
                .list_trace_spans("request-7")
                .await
                .expect("trace spans should load")
                .is_empty()
        );
    }
}
