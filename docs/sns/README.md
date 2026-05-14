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

List subscriptions and update subscription attributes:

```sh
aws --endpoint-url http://localhost:4566 sns list-subscriptions

aws --endpoint-url http://localhost:4566 sns set-subscription-attributes \
  --subscription-arn arn:aws:sns:us-east-1:000000000000:local-events:subscription-id \
  --attribute-name RawMessageDelivery \
  --attribute-value true
```

## Terraform Notes

The SNS implementation is intentionally shaped around SDK and Terraform
workflows that create a topic, refresh topic attributes, list tags, manage basic
subscriptions, and publish to SQS. The following Terraform-style paths are
covered by the current API surface:

- Topic create/read/delete through `CreateTopic`, `GetTopicAttributes`,
  `ListTopics`, and `DeleteTopic`.
- Topic tags through create-time tags, `ListTagsForResource`, `TagResource`,
  and `UntagResource`.
- Topic policy persistence through `Policy` in `CreateTopic` and
  `SetTopicAttributes`.
- SQS subscriptions through `Subscribe`, `GetSubscriptionAttributes`,
  `ListSubscriptions`, `ListSubscriptionsByTopic`, `SetSubscriptionAttributes`,
  and `Unsubscribe`.

Filtering, confirmation handshakes, non-SQS protocols, and full FIFO behavior
are still out of scope.

## Authorization

SNS currently inherits the same authorization modes as other Hiraeth services
through `HIRAETH_AUTH_MODE`:

| Mode | Behavior |
| --- | --- |
| `audit` | Evaluates policies, logs the decision, and allows the request. |
| `enforce` | Enforces policy decisions and denies by default. |
| `off` | Skips authorization checks entirely. |

The default mode is `audit`. SNS actions participate in the same IAM identity
policy evaluation as other services, and topic-scoped actions can also include a
stored topic resource policy in the authorization check. Policy behavior is
still intentionally smaller than AWS, but it is useful for local allow/deny
testing.

## Web UI

The web UI is an admin/debug surface for local SNS state. Current SNS UI
coverage includes:

- Topic browsing with account, region, and prefix filters.
- Topic detail pages with subscription list, publish form, and subscribe form.
- Topic creation and deletion.
- Subscription creation and deletion, including raw message delivery display and
  toggling.
- Topic tag inspection and management through the topic detail view.
- Topic policy inspection through the topic detail view.
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
| `ListSubscriptions` | Supported | Lists account subscriptions for the current region with simple local pagination tokens. |
| `ListSubscriptionsByTopic` | Supported | Lists subscriptions stored for a topic with simple local pagination tokens. |
| `ListTagsForResource` | Supported | Returns stored tags for a topic ARN. |
| `ListTopics` | Supported | Lists topics for the current account and region with simple local pagination tokens. |
| `SetSubscriptionAttributes` | Partial | Updates stored subscription attributes including raw delivery and filter/redrive JSON. |
| `SetTopicAttributes` | Partial | Updates supported topic attributes and validates JSON policy-shaped attributes. |
| `TagResource` | Supported | Upserts topic tags and enforces basic tag limits. |
| `UntagResource` | Supported | Removes requested topic tag keys. |
| `Unsubscribe` | Supported | Deletes a stored subscription. |

## Current Gaps

- Only the `sqs` delivery protocol is supported. HTTP/HTTPS, email, SMS, Lambda,
  and other protocols are not implemented.
- Subscription confirmation is not modeled. All subscriptions are treated as
  confirmed.
- Topic policy evaluation is still limited compared with AWS.
- Message filtering is not supported.
- FIFO topic behavior is not implemented beyond storing selected attributes.
- Topic archive/data protection/delivery policies are stored where supported,
  but their service-side behavior is not modeled.
- The web UI is a local admin preview and is not authenticated.
