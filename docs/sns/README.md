# SNS

Hiraeth includes a lightweight SNS implementation focused on topics, subscriptions,
and message publishing. The current scope supports SQS protocol delivery and a
web admin UI for managing local topic state.

This is not full SNS parity. It is intended for local integration tests and
development workflows that need a small pub/sub surface.

## Quickstart

With Hiraeth running locally and the default `test` / `test` credential
configured:

```sh
export AWS_ACCESS_KEY_ID=test
export AWS_SECRET_ACCESS_KEY=test
export AWS_DEFAULT_REGION=us-east-1
```

Create a topic:

```sh
aws --endpoint-url http://localhost:4566 sns create-topic \
  --name local-events
```

Subscribe an SQS queue to that topic:

```sh
aws --endpoint-url http://localhost:4566 sns subscribe \
  --topic-arn arn:aws:sns:us-east-1:000000000000:local-events \
  --protocol sqs \
  --notification-endpoint arn:aws:sqs:us-east-1:000000000000:local-queue
```

Publish a message:

```sh
aws --endpoint-url http://localhost:4566 sns publish \
  --topic-arn arn:aws:sns:us-east-1:000000000000:local-events \
  --message "hello from hiraeth"
```

## Authorization

SNS currently inherits the same authorization modes as other Hiraeth services
through `HIRAETH_AUTH_MODE`:

| Mode | Behavior |
| --- | --- |
| `audit` | Evaluates policies, logs the decision, and allows the request. |
| `enforce` | Enforces policy decisions and denies by default. |
| `off` | Skips authorization checks entirely. |

The default mode is `audit`. SNS topic policy evaluation is planned but not yet
implemented. In the meantime, IAM identity policies are evaluated for
SNS-scoped actions in `enforce` mode.

## Web UI

The web UI is an admin/debug surface for local SNS state. Current SNS UI
coverage includes:

- Topic browsing with account, region, and prefix filters.
- Topic detail pages with subscription list, publish form, and subscribe form.
- Live-updating topic counts and subscription stats on the dashboard.
- Topic creation and deletion.
- Subscription creation and deletion.
- A read-only JSON API endpoint for topic lists.

The web UI does not use SigV4 authentication. Keep `HIRAETH_WEB_HOST` bound to a
trusted interface unless you intentionally want to expose local test state.

## API Support

Status labels:

- `Supported`: implemented and covered by unit and/or AWS SDK integration tests.
- `Partial`: implemented, but known AWS edge behavior is incomplete.
- `Not implemented`: requests currently return `UnsupportedOperation`.

| API | Status | Notes |
| --- | --- | --- |
| `CreateTopic` | Supported | Creates a topic with name, region, account, and optional display name. |
| `Publish` | Partial | Publishes to all SQS subscriptions. Subject and message body are supported. Other protocols are not yet implemented. |
| `Subscribe` | Partial | Creates an SQS subscription to a topic. Other protocols are not yet implemented. |
| `ConfirmSubscription` | Not implemented | Subscriptions are created in a confirmed state. |
| `DeleteTopic` | Not implemented | Topic deletion is available through the web UI but not the Query API yet. |
| `GetSubscriptionAttributes` | Not implemented | Subscriptions can be inspected in the web UI. |
| `GetTopicAttributes` | Not implemented | Topic attributes can be inspected in the web UI. |
| `ListSubscriptions` | Not implemented | Subscriptions can be inspected in the web UI. |
| `ListSubscriptionsByTopic` | Not implemented | Subscriptions can be inspected in the web UI. |
| `ListTopics` | Not implemented | Topics can be inspected in the web UI and through the JSON API. |
| `SetSubscriptionAttributes` | Not implemented | Subscription attributes are not modeled yet. |
| `SetTopicAttributes` | Not implemented | Topic attributes are not modeled yet. |
| `Unsubscribe` | Not implemented | Subscription deletion is available through the web UI but not the Query API yet. |

## Current Gaps

- Only the `sqs` delivery protocol is supported. HTTP/HTTPS, email, SMS, Lambda,
  and other protocols are not implemented.
- Subscription confirmation is not modeled. All subscriptions are treated as
  confirmed.
- Topic policies and subscription attributes are not evaluated or modifiable
  through the Query API yet.
- Message filtering and subscription attributes are not supported.
- FIFO topics are not supported.
- The web UI is a local admin preview and is not authenticated.
