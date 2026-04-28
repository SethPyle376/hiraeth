CREATE TABLE hiraeth_trace_request (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    request_id TEXT NOT NULL,
    started_at TEXT NOT NULL,
    completed_at TEXT NOT NULL,
    duration_ms INTEGER NOT NULL,
    auth_ms INTEGER NOT NULL,
    route_ms INTEGER,
    service TEXT,
    region TEXT,
    account_id TEXT,
    principal TEXT,
    access_key TEXT,
    method TEXT NOT NULL,
    host TEXT NOT NULL,
    path TEXT NOT NULL,
    query TEXT,
    request_headers_json TEXT NOT NULL,
    request_body BLOB NOT NULL,
    response_status_code INTEGER NOT NULL,
    response_headers_json TEXT NOT NULL,
    response_body BLOB NOT NULL,
    error_message TEXT
);

CREATE INDEX idx_hiraeth_trace_request_started_at
    ON hiraeth_trace_request(started_at);

CREATE UNIQUE INDEX idx_hiraeth_trace_request_request_id
    ON hiraeth_trace_request(request_id);

CREATE INDEX idx_hiraeth_trace_request_service
    ON hiraeth_trace_request(service);

CREATE INDEX idx_hiraeth_trace_request_principal
    ON hiraeth_trace_request(account_id, principal);

CREATE TABLE hiraeth_trace_span (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    request_id TEXT NOT NULL,
    span_id TEXT NOT NULL,
    parent_span_id TEXT,
    name TEXT NOT NULL,
    layer TEXT NOT NULL,
    started_at TEXT NOT NULL,
    completed_at TEXT NOT NULL,
    duration_ms INTEGER NOT NULL,
    status TEXT NOT NULL,
    attributes_json TEXT NOT NULL
);

CREATE INDEX idx_hiraeth_trace_span_request_id
    ON hiraeth_trace_span(request_id);

CREATE INDEX idx_hiraeth_trace_span_parent_span_id
    ON hiraeth_trace_span(parent_span_id);
