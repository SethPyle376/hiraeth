# Changelog

All notable changes to Hiraeth will be documented in this file.

## 0.2.1 - 2026-04-29

Tracing and local request inspection slice.

### Added

- SQLite-backed request tracing for handled AWS endpoint requests.
- Trace ids now use the generated AWS request id for consistent log, response,
  and web UI correlation.
- Runtime spans for authentication, IAM identity resolution, routing,
  authorization resolution/evaluation, service handling, action handling, and
  selected action-specific operations.
- Trace span parent ids, enabling the web UI to render the request-processing
  flow as a directed graph.
- Web UI tracing dashboard with request id search, service/action/status
  filters, trace clearing, and recent request summaries.
- Web UI trace detail pages with a request-flow graph, selectable span details,
  copied trace ids, full request/response headers, and full request/response
  bodies.
- Documentation for tracing behavior, captured data, storage, and current
  limitations.

### Changed

- AWS action handling now serializes typed action responses centrally instead
  of each action manually rendering its service response.
- Authorization tracing now records the real allow/deny decision even when
  `HIRAETH_AUTH_MODE=audit` allows the request to continue.

### Known Gaps

- Tracing is Hiraeth-specific and does not currently export OpenTelemetry data.
- Trace storage has no automatic retention or sampling configuration yet.
- Full request and response bodies are stored intentionally for local debugging;
  keep the web UI and SQLite database bound to trusted local workflows.

## 0.2.0 - 2026-04-28

IAM identity-policy slice and expanded local administration UI.

### Added

- IAM Query API support for local user and policy workflows:
  - `CreateAccessKey`
  - `CreatePolicy`
  - `CreateUser`
  - `DeletePolicy`
  - `DeleteUser`
  - `GetUser`
  - `PutUserPolicy`
  - `AttachUserPolicy`
  - `DetachUserPolicy`
- STS `GetCallerIdentity` support for SDK and CLI checks against the local
  endpoint.
- SQLite-backed IAM users, access keys, inline user policies, managed policies,
  and managed policy attachments.
- Identity policy evaluation for inline and attached managed user policies.
- IAM admin UI for reviewing principals, access keys, inline policies, managed
  policies, and policy attachments.
- Web UI controls for creating/deleting principals, creating/deleting access
  keys, setting inline policies, creating/deleting managed policies, editing
  managed policy documents, and attaching/detaching managed policies.
- Documentation for current IAM API support and known IAM limitations.

### Changed

- Authorization now combines SQS queue resource policies with IAM identity
  policies where supported.
- The seeded local `test` user now has an inline account-admin policy for a
  smoother first-run local workflow.
- Web UI assets are vendored and built into the binary instead of loaded from a
  CDN.
- First-party web assets now revalidate instead of using immutable caching with
  non-fingerprinted URLs.
- Web UI layout and shared components were refined for SQS and IAM detail
  pages.

### Known Gaps

- IAM support is intentionally partial. Roles, groups, managed policy versions,
  policy listing APIs, policy retrieval APIs, and assume-role flows are not
  implemented yet.
- Policy evaluation supports the current local identity/resource-policy needs,
  but AWS IAM condition keys and full cross-policy semantics are not complete.
- STS support is limited to `GetCallerIdentity` and currently assumes the
  authenticated principal is a local user.

## 0.1.2 - 2026-04-20

Resource-policy authorization slice for local SQS testing.

### Added

- Configurable authorization modes through `HIRAETH_AUTH_MODE`, defaulting to
  `audit`.
- Queue resource policy evaluation for SQS requests with wildcard support for
  actions, resources, and principals.
- Integration coverage for `enforce` mode using the AWS Rust SDK client against
  a running Hiraeth endpoint.

### Changed

- README documentation now covers authorization modes, queue policy examples,
  and the current limits of `enforce` mode.
- SQS queue policies are no longer just persisted metadata; they are now used
  during queue-scoped authorization checks.

### Known Gaps

- This slice is queue resource policy authorization, not full IAM parity.
  Identity policies, conditions, and cross-policy evaluation are still future
  work.

## 0.1.1 - 2026-04-16

Maintenance release focused on the web admin UI, local demo data, and release
workflow polish.

### Added

- `cargo run -p xtask -- seed` for seeding a running Hiraeth endpoint through
  the AWS Rust SDK.
- Seed data for standard queues, a FIFO queue, a dead-letter queue, queue tags,
  message attributes, `AWSTraceHeader`, delayed messages, and an in-flight
  message.
- Web UI forms for creating standard and FIFO queues from the SQS dashboard and
  queue browser.
- Web UI queue actions for sending messages, purging queues, deleting queues,
  deleting stored messages, and managing queue tags.
- Auto-refreshing HTMX fragments for dashboard stats, queue lists, queue detail
  stats, and message lists.
- Dismissible success/error banners and inline validation feedback for web UI
  actions.
- Copy buttons for queue ARN and queue URL in the queue detail page.
- `rust-toolchain.toml` pinning the workspace toolchain to Rust 1.95.0.
- `scripts/publish-image.sh` for local multi-architecture GHCR image publishing.

### Changed

- Refined the web UI layout, landing copy, queue detail layout, message
  attribute rendering, collapsible panels, and README screenshot.
- Runtime now passes the configured AWS endpoint URL into the web UI so queue URL
  rendering follows the running emulator configuration.
- CI now uses the pinned Rust toolchain and runs an amd64 Docker build with image
  size reporting instead of publishing images from GitHub Actions.
- Docker builds now install musl targets through the pinned workspace toolchain,
  which fixes target installation for static multi-architecture builds.
- Docker Compose no longer declares the old named SQLite volume by default.
- README container publishing instructions now describe local release publishing.

### Fixed

- Reworked message attribute sorting to satisfy the pinned clippy configuration.

## 0.1.0 - 2026-04-15

Initial preview release focused on local SQS emulation for integration tests.

### Added

- AWS SigV4 header authentication for SDK-compatible local requests.
- Seeded local test principal and access key using `test` / `test`.
- SQLite-backed storage for principals, access keys, SQS queues, messages, attributes, and queue tags.
- SQS queue APIs:
  - `CreateQueue`
  - `DeleteQueue`
  - `GetQueueAttributes`
  - `GetQueueUrl`
  - `ListQueues`
  - `ListQueueTags`
  - `PurgeQueue`
  - `SetQueueAttributes`
  - `TagQueue`
  - `UntagQueue`
- SQS message APIs:
  - `ChangeMessageVisibility`
  - `ChangeMessageVisibilityBatch`
  - `DeleteMessage`
  - `DeleteMessageBatch`
  - `ReceiveMessage`
  - `SendMessage`
  - `SendMessageBatch`
- Message attribute MD5 calculation compatible with AWS SDK client validation.
- Receive message support for visibility timeout, wait time polling, message attributes, and selected system attributes including `AWSTraceHeader`.
- JSON AWS error rendering for common SDK-compatible error paths.
- Request tracing and structured logging.
- Web admin UI for inspecting SQS queues, messages, attributes, tags, receipt handles, and timing metadata.
- Dockerfile and Docker Compose support.
- GitHub Actions CI for formatting, clippy, tests, SQLx offline metadata checks, and tagged multi-architecture GHCR image publishing.
- README quickstart, API support matrix, known gaps, AI usage statement, and MIT license.

### Known Gaps

- IAM, authorization, and queue policy enforcement are not implemented yet.
- FIFO queues store FIFO-related fields, but ordering, deduplication windows, and throughput behavior are not fully modeled.
- AWS error and validation parity is pragmatic, not exhaustive.
- Redrive task APIs and dead-letter source queue listing are not implemented.
- The web UI is a local admin/debug preview and is not authenticated.
- Web UI assets currently load from public CDNs.
