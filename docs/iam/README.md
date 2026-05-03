# IAM

Hiraeth includes a focused IAM implementation for local integration tests. The
current scope is user-centered: users, access keys, inline user policies,
managed policies, managed policy attachments, and identity policy evaluation.

This is not full IAM parity. It is intended to support local test credentials,
policy-driven authorization checks, and Terraform or SDK workflows that need a
small IAM surface.

## Quickstart

With Hiraeth running locally and the default `test` / `test` credential
configured:

```sh
export AWS_ACCESS_KEY_ID=test
export AWS_SECRET_ACCESS_KEY=test
export AWS_DEFAULT_REGION=us-east-1
```

Create a user:

```sh
aws --endpoint-url http://localhost:4566 iam create-user \
  --user-name local-app
```

Create an access key for that user:

```sh
aws --endpoint-url http://localhost:4566 iam create-access-key \
  --user-name local-app
```

Create and attach a managed policy:

```sh
cat > /tmp/hiraeth-sqs-policy.json <<'JSON'
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": "sqs:*",
      "Resource": "arn:aws:sqs:us-east-1:000000000000:*"
    }
  ]
}
JSON

aws --endpoint-url http://localhost:4566 iam create-policy \
  --policy-name local-sqs-admin \
  --policy-document file:///tmp/hiraeth-sqs-policy.json

aws --endpoint-url http://localhost:4566 iam attach-user-policy \
  --user-name local-app \
  --policy-arn arn:aws:iam::000000000000:policy/local-sqs-admin
```

Check the authenticated identity:

```sh
aws --endpoint-url http://localhost:4566 sts get-caller-identity
```

## Authorization

Hiraeth supports three authorization modes through `HIRAETH_AUTH_MODE`:

| Mode | Behavior |
| --- | --- |
| `audit` | Evaluates policies, logs the decision, and allows the request. |
| `enforce` | Enforces policy decisions and denies by default. |
| `off` | Skips authorization checks entirely. |

The default mode is `audit`.

For v0.2, identity policies are evaluated for local IAM users. Inline user
policies and attached managed policies can allow or deny actions. SQS queue
resource policies are also evaluated for queue-scoped SQS actions.

Policy support is intentionally pragmatic:

- `Allow` and `Deny` effects are supported.
- Wildcards are supported in actions, resources, and principals.
- Identity policy statements do not need a `Principal` field.
- Resource policy statements can include AWS principals.
- Conditions are not implemented yet.
- Full AWS IAM policy evaluation semantics are not complete yet.

## Web UI

The web UI is an admin/debug surface for local IAM state. Current IAM UI
coverage includes:

- Principal browsing and detail pages.
- Access key creation, deletion, and masked secret-key display.
- Inline policy creation, replacement, deletion, and JSON inspection.
- Managed policy browsing and detail pages.
- Managed policy document editing.
- Managed policy attachment and detachment for users.

The web UI does not use SigV4 authentication. Keep `HIRAETH_WEB_HOST` bound to a
trusted interface unless you intentionally want to expose local test state.

## API Support

Status labels:

- `Supported`: implemented and covered by unit and/or AWS SDK integration tests.
- `Partial`: implemented, but known AWS edge behavior is incomplete.
- `Not implemented`: requests currently return `UnsupportedOperation`.

### IAM

| API | Status | Notes |
| --- | --- | --- |
| `AttachUserPolicy` | Supported | Attaches an existing managed policy to a local user. Policy ARN path and account are respected. |
| `CreateAccessKey` | Supported | Creates an access key for an existing local user. Secret key is generated and persisted locally. |
| `CreatePolicy` | Partial | Creates a customer managed policy with path and document storage. Policy versions are not modeled. |
| `CreateUser` | Partial | Creates a local IAM user with path and generated user id. Validation is pragmatic. |
| `DeletePolicy` | Partial | Deletes a customer managed policy. AWS delete constraints such as non-default versions are not modeled. |
| `DeleteUser` | Partial | Deletes a local user. Related local credentials and inline policies are removed by the store. |
| `DetachUserPolicy` | Supported | Detaches a managed policy from a local user. |
| `GetUser` | Supported | Returns the requested user, or the authenticated user when `UserName` is omitted. |
| `PutUserPolicy` | Partial | Creates or replaces an inline user policy. Policy document validation is JSON-focused, not full IAM grammar validation. |
| `AddUserToGroup` | Not implemented | Groups are not implemented yet. |
| `CreateGroup` | Not implemented | Groups are not implemented yet. |
| `CreateRole` | Not implemented | Roles and assume-role flows are future work. |
| `DeleteAccessKey` | Not implemented | Access key deletion exists in the web UI store path but not the IAM Query API yet. |
| `DeleteUserPolicy` | Not implemented | Inline policy deletion exists in the web UI store path but not the IAM Query API yet. |
| `GetPolicy` | Not implemented | Managed policy documents can be inspected in the web UI. |
| `GetUserPolicy` | Not implemented | Inline policy documents can be inspected in the web UI. |
| `ListAccessKeys` | Supported | Lists access keys for a user. Defaults to the signing user when `UserName` is omitted. |
| `ListAttachedUserPolicies` | Supported | Lists managed policies attached to a user. Defaults to the signing user when `UserName` is omitted. |
| `ListPolicies` | Not implemented | Managed policies can be inspected in the web UI. |
| `ListUserPolicies` | Not implemented | Inline policies can be inspected in the web UI. |
| `ListUsers` | Not implemented | Users can be inspected in the web UI. |

### STS

| API | Status | Notes |
| --- | --- | --- |
| `GetCallerIdentity` | Supported | Returns account, ARN, and user id for the authenticated local user. |
| `AssumeRole` | Not implemented | Roles and temporary credentials are future work. |

## Current Gaps

- IAM roles, groups, instance profiles, managed policy versions, and temporary
  credential flows are not implemented yet.
- Policy conditions are not implemented yet.
- Policy validation is intentionally light. Hiraeth stores JSON policy
  documents and evaluates the subset it currently understands.
- IAM list/get/delete APIs are incomplete. Some data is currently available
  through the web UI before it is available through the IAM Query API.
- STS support is limited to `GetCallerIdentity`.
