# Documentation

Service-specific documentation lives under a directory per service. Cross-cutting
runtime documentation lives beside those service directories.

## Services

- [IAM](iam/README.md): user, access key, inline policy, managed policy,
  authorization, web UI, and API support notes.
- [SQS](sqs/README.md): queue auth modes, CLI examples, web UI notes, API support, and current gaps.

## Runtime

- [Tracing](tracing/README.md): SQLite-backed request traces, span flow,
  captured request/response data, web UI usage, and current limits.

Additional service directories can follow the same layout as new emulation
targets come online.
