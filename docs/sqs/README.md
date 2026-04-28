# SQS

SQS was Hiraeth's first service target and remains the most complete service
surface. This page collects service-specific notes that are likely to change as
SQS compatibility improves.

## Quickstart

With Hiraeth running locally and the default `test` / `test` credential
configured:

```sh
export AWS_ACCESS_KEY_ID=test
export AWS_SECRET_ACCESS_KEY=test
export AWS_DEFAULT_REGION=us-east-1
```

Create and inspect a queue with the AWS CLI:

```sh
aws --endpoint-url http://localhost:4566 sqs create-queue --queue-name local-orders
aws --endpoint-url http://localhost:4566 sqs list-queues
aws --endpoint-url http://localhost:4566 sqs send-message \
  --queue-url http://localhost:4566/000000000000/local-orders \
  --message-body "hello from hiraeth"
aws --endpoint-url http://localhost:4566 sqs receive-message \
  --queue-url http://localhost:4566/000000000000/local-orders \
  --message-attribute-names All
```

## Authorization

Hiraeth currently evaluates IAM identity policies and SQS queue resource
policies. The default mode is `audit`, which logs authorization decisions but
still allows the request.

| Mode | Behavior |
| --- | --- |
| `audit` | Evaluates policies, logs the result, and allows the request. |
| `enforce` | Requires a matching policy decision and denies by default. |
| `off` | Skips authorization checks entirely. |

Identity policies are evaluated for local IAM users. Queue resource policies
are evaluated for queue-scoped requests against existing queues. Policy
conditions and full AWS IAM semantics are still incomplete, but `enforce` mode
can now authorize both queue operations and non-queue-scoped requests such as
`CreateQueue` through IAM identity policies.

Allow `test` to send messages to `local-orders`:

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Principal": {
        "AWS": "arn:aws:iam::000000000000:user/test"
      },
      "Action": "sqs:SendMessage",
      "Resource": "arn:aws:sqs:us-east-1:000000000000:local-orders"
    }
  ]
}
```

Deny message deletion for everyone on that same queue:

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Deny",
      "Principal": "*",
      "Action": "sqs:DeleteMessage",
      "Resource": "arn:aws:sqs:us-east-1:000000000000:local-orders"
    }
  ]
}
```

You can apply a policy through `SetQueueAttributes`:

```sh
POLICY=$(cat <<'JSON'
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Principal": {
        "AWS": "arn:aws:iam::000000000000:user/test"
      },
      "Action": "sqs:SendMessage",
      "Resource": "arn:aws:sqs:us-east-1:000000000000:local-orders"
    }
  ]
}
JSON
)

aws --endpoint-url http://localhost:4566 sqs set-queue-attributes \
  --queue-url http://localhost:4566/000000000000/local-orders \
  --attributes Policy="$POLICY"
```

## Web UI

The web UI is an admin/debug surface for local service state. The current SQS
UI supports queue browsing, queue details, message inspection, attributes, tags,
purge, delete queue, and delete message.

The web UI does not use SigV4 authentication. Keep `HIRAETH_WEB_HOST` bound to a
trusted interface unless you intentionally want to expose local test state.

The current UI vendors its JavaScript and CSS assets and serves them from the
Hiraeth web process.

## API Support

Status labels:

- `Supported`: implemented and covered by unit and/or AWS SDK integration tests.
- `Partial`: implemented, but known AWS edge behavior is incomplete.
- `Not implemented`: requests currently return `UnsupportedOperation`.

| API | Status | Notes |
| --- | --- | --- |
| `ChangeMessageVisibility` | Supported | Updates visibility timeout for a receipt handle. |
| `ChangeMessageVisibilityBatch` | Supported | Returns per-entry success/failure records. |
| `CreateQueue` | Partial | Supports attributes and tags. Queue validation exists, but AWS parity is not exhaustive. |
| `DeleteMessage` | Supported | Deletes by queue URL and receipt handle. |
| `DeleteMessageBatch` | Supported | Returns per-entry success/failure records. |
| `DeleteQueue` | Supported | Deletes queue and cascades stored messages/tags. |
| `GetQueueAttributes` | Supported | Supports the queue attributes modeled by Hiraeth. |
| `GetQueueUrl` | Supported | Supports owner account override. |
| `ListQueues` | Supported | Supports prefix, max results, and next token. |
| `ListQueueTags` | Supported | Returns stored queue tags. |
| `PurgeQueue` | Supported | Deletes stored messages for the queue. |
| `ReceiveMessage` | Partial | Supports max messages, visibility timeout, wait time polling, message attributes, and `AWSTraceHeader`. FIFO ordering semantics are not complete. |
| `SendMessage` | Partial | Supports body, delay, message attributes, system attributes, and FIFO metadata storage. Full FIFO deduplication semantics are not complete. |
| `SendMessageBatch` | Partial | Supports per-entry success/failure shape and message attributes. Full FIFO semantics are not complete. |
| `SetQueueAttributes` | Supported | Updates modeled queue attributes, including queue resource policies used for authorization. |
| `TagQueue` | Supported | Upserts queue tags and enforces basic tag limits. |
| `UntagQueue` | Supported | Removes requested tag keys. |
| `AddPermission` | Not implemented | Queue policy evaluation exists, but the `AddPermission` helper API is not implemented. |
| `CancelMessageMoveTask` | Not implemented | Redrive task APIs are out of scope for the first release. |
| `ListDeadLetterSourceQueues` | Not implemented | Redrive behavior is not complete yet. |
| `ListMessageMoveTasks` | Not implemented | Redrive task APIs are out of scope for the first release. |
| `RemovePermission` | Not implemented | Queue policy evaluation exists, but the `RemovePermission` helper API is not implemented. |
| `StartMessageMoveTask` | Not implemented | Redrive task APIs are out of scope for the first release. |

## Current Gaps

- Queue resource policies and IAM user identity policies are evaluated for the
  supported local authorization path, but policy conditions and full AWS IAM
  cross-policy semantics are not complete yet.
- Error responses are SDK-compatible for common paths, but not exhaustively
  identical to AWS.
- Request validation is pragmatic and still needs a deeper AWS parity pass.
- FIFO behavior stores FIFO fields, but does not yet fully model ordering,
  deduplication windows, or throughput behavior.
- The web UI is a local admin preview and is not authenticated.
