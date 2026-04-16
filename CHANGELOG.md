# Changelog

All notable changes to Hiraeth will be documented in this file.

## 0.1.0 - Unreleased

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
- GitHub Actions CI for formatting, clippy, tests, and SQLx offline metadata checks.
- Local release script for publishing multi-architecture GHCR images.
- README quickstart, API support matrix, known gaps, AI usage statement, and MIT license.

### Known Gaps

- IAM, authorization, and queue policy enforcement are not implemented yet.
- FIFO queues store FIFO-related fields, but ordering, deduplication windows, and throughput behavior are not fully modeled.
- AWS error and validation parity is pragmatic, not exhaustive.
- Redrive task APIs and dead-letter source queue listing are not implemented.
- The web UI is a local admin/debug preview and is not authenticated.
- Web UI assets currently load from public CDNs.
