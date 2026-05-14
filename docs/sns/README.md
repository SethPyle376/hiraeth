# SNS

Hiraeth includes a lightweight SNS implementation focused on topics,
subscriptions, tags, and message publishing. The current scope supports SQS
protocol delivery and a web admin UI for managing local topic state.

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

Create a topic with tags:

```sh
aws --endpoint-url http://localhost:4566 sns create-topic \
  --name local-events \
  --tags Key=environment,Value=test Key=owner,Value=hiraeth
```

Subscribe an SQS queue to that topic:

```sh
aws --endpoint-url http://localhost:4566 sns subscribe \
  --topic-arn arn:aws:sns:us-east-1:000000000000:local-events \
  --protocol sqs \
  --notification-endpoint arn:aws:sqs:us-east-1:000000000000:local-queue
```

Subscribe with raw message delivery (delivers the raw body instead of the SNS JSON wrapper):

```sh
aws --endpoint-url http://localhost:4566 sns subscribe \
  --topic-arn arn:aws:sns:us-east-1:000000000000:local-events \
  --protocol sqs \
  --notification-endpoint arn:aws:sqs:us-east-1:000000000000:local-queue \
  --attributes RawMessageDelivery=true
```

Publish a message:

```sh
aws --endpoint-url http://localhost:4566 sns publish \
  --topic-arn arn:aws:sns:us-east-1:000000000000:local-events \
  --message "hello from hiraeth"
```

List topics and tags:

```sh
aws --endpoint-url http://localhost:4566 sns list-topics

aws --endpoint-url http://localhost:4566 sns list-tags-for-resource \
  --resource-arn arn:aws:sns:us-east-1:000000000000:local-events
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
- Topic creation and deletion.
- Subscription creation and deletion, including a raw message delivery option.
- Topic tag inspection and management through the topic detail view.
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
| `CreateTopic` | Supported | Creates a topic with attributes and optional tags. |
| `Publish` | Partial | Publishes to all SQS subscriptions. Subject and message body are supported. Other protocols and message filtering are not yet implemented. |
| `Subscribe` | Partial | Creates an SQS subscription to a topic. Subscription attributes are parsed and stored. Other protocols are not yet implemented. |
| `ConfirmSubscription` | Not implemented | Subscriptions are created in a confirmed state. |
| `DeleteTopic` | Supported | Deletes a topic and removes stored subscriptions and tags for it. |
| `GetSubscriptionAttributes` | Supported | Returns stored subscription metadata and parsed subscription attributes. |
| `GetTopicAttributes` | Supported | Returns stored topic metadata and topic attributes. |
| `ListSubscriptions` | Not implemented | Subscriptions can be inspected in the web UI. |
| `ListSubscriptionsByTopic` | Supported | Lists subscriptions stored for a topic. Pagination is not implemented yet. |
| `ListTagsForResource` | Supported | Returns stored tags for a topic ARN. |
| `ListTopics` | Supported | Lists topics for the current account and region with simple local pagination tokens. |
| `SetSubscriptionAttributes` | Not implemented | Subscription attributes can be set at creation time but not updated afterward yet. |
| `SetTopicAttributes` | Partial | Updates supported topic attributes. Validation is intentionally narrower than AWS. |
| `TagResource` | Supported | Upserts topic tags and enforces basic tag limits. |
| `UntagResource` | Supported | Removes requested topic tag keys. |
| `Unsubscribe` | Supported | Deletes a stored subscription. |

## Current Gaps

- Only the `sqs` delivery protocol is supported. HTTP/HTTPS, email, SMS, Lambda,
  and other protocols are not implemented.
- Subscription confirmation is not modeled. All subscriptions are treated as
  confirmed.
- Topic policy evaluation is still limited compared with AWS.
- Subscription attributes can be set at creation time but cannot be updated
  afterward through `SetSubscriptionAttributes` yet.
- Message filtering is not supported.
- FIFO topic behavior is not implemented beyond storing selected attributes.
- `ListSubscriptionsByTopic` pagination tokens are not implemented yet.
- The web UI is a local admin preview and is not authenticated.
