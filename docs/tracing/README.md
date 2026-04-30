# Tracing

Hiraeth includes a local tracing system for inspecting requests that flow
through the AWS-compatible endpoint. Traces are stored in the same SQLite
database as service state and are available from the web UI.

This is intentionally not a full OpenTelemetry pipeline. The goal is a small,
zero-extra-infrastructure view into local test traffic: what request arrived,
how Hiraeth routed it, how authentication and authorization evaluated it, which
action handled it, and what response was returned.

## What Is Captured

Each handled AWS request records a request trace with:

- Request id.
- HTTP method, host, path, query string, headers, and body.
- Response status, headers, and body.
- Service, region, account, principal, and access key when resolved.
- Total duration plus authentication and routing timing.
- Error message when request handling fails before a normal service response.

Each request can also include spans for the request-processing flow:

```text
request.handle
  authn.authenticate
    iam.resolve_identity
      authz.evaluate
        action.handle
          action-specific spans
```

Spans include their own ids, parent ids, names, categories, timing, status, and
attributes. Action spans carry the resolved action name, which powers the action
filter in the UI.

## Web UI

Open the tracing dashboard from the web UI navigation or visit:

```text
http://localhost:4567/traces
```

The trace list supports:

- Search by request id.
- Filtering by service, action, and response status class.
- Reviewing recent request summaries.
- Clearing stored traces.

The trace detail page shows:

- A directed request-flow graph.
- Span status coloring and selected-span details.
- Full request and response headers.
- Full request and response bodies.
- A copy action for the trace/request id.

## Data Exposure

Hiraeth stores full request and response bodies by design. This makes local
debugging much easier, especially when testing SDK clients, Terraform runs, and
authorization behavior.

It also means traces may include credentials, message bodies, policy documents,
and other test payloads. Treat the web UI and SQLite database as local
development artifacts. Do not expose them to untrusted networks or long-lived
shared environments unless that is an intentional part of your workflow.

## Storage And Retention

Traces are persisted in SQLite using the configured `HIRAETH_DATABASE_URL`.
They survive process restarts when the database file is retained.

There is no automatic retention policy yet. Use the web UI's clear action to
delete stored traces when they are no longer useful.

## Current Gaps

- Tracing is Hiraeth-specific and does not currently export OTEL data.
- There is no retention or sampling configuration yet.
- Trace filtering is intentionally simple: request id search plus service,
  action, and status filters.
- Span coverage is focused on the main request path and selected action-level
  operations. More service-specific spans will be added as new workflows need
  them.
