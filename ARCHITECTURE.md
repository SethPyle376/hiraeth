# Architecture

This document describes the architecture, design patterns, and key abstractions in Hiraeth.

## Overview

Hiraeth is a local AWS emulator designed for fast integration testing. It implements AWS-compatible HTTP endpoints that respond to signed SDK requests, storing state in SQLite.

**Design Goals:**
- Fast local development and testing
- AWS SDK compatibility
- Type-safe request/response handling
- Pluggable storage backend
- Clear separation of concerns

## Crate Structure

Hiraeth uses a Rust workspace with focused, single-responsibility crates:

```
hiraeth_http            → HTTP primitives (request/response types)
hiraeth_store           → Storage trait definitions
hiraeth_store_sqlx      → SQLite implementation via SQLx
hiraeth_core            → Shared types, protocol parsing, error handling
hiraeth_auth            → AWS SigV4 authentication
hiraeth_router          → Service routing and authorization orchestration
hiraeth_sqs             → SQS service implementation
hiraeth_iam             → IAM service implementation
hiraeth_web             → Admin web UI (Askama templates + HTMX)
hiraeth_runtime         → Binary entry point, configuration, HTTP server
hiraeth_integration_tests → End-to-end tests with real AWS SDKs
xtask                   → Development automation tasks
```

**Dependency Flow:**

```
runtime
  ├─> web
  ├─> router ──> auth
  ├─> sqs ──┐
  └─> iam ──┼─> core ──┬─> http
            │          └─> store ──> store_sqlx
            └─> store
```

This unidirectional flow prevents circular dependencies and makes the codebase easier to reason about.

## Key Design Patterns

### 1. TypedAwsAction Pattern

Individual AWS operations are implemented using the `TypedAwsAction` trait, which provides type-safe request/response handling.

**Definition:**

```rust
#[async_trait]
pub trait TypedAwsAction<S>: Send + Sync {
    type Request: DeserializeOwned + Send;
    type Error: Into<ServiceResponse> + Send;

    fn name(&self) -> &'static str;
    fn payload_format(&self) -> AwsActionPayloadFormat;
    fn parse_error(&self, error: AwsActionPayloadParseError) -> Self::Error;

    async fn handle(
        &self,
        request: ResolvedRequest,
        payload: Self::Request,
        store: &S,
    ) -> Result<ServiceResponse, Self::Error>;

    async fn resolve_authorization_typed(
        &self,
        request: &ResolvedRequest,
        payload: Self::Request,
        store: &S,
    ) -> Result<AuthorizationCheck, Self::Error>;
}
```

**Implementation Example:**

```rust
pub(crate) struct CreateQueueAction;

#[derive(Deserialize)]
struct CreateQueueRequest {
    queue_name: String,
    attributes: HashMap<String, String>,
    tags: HashMap<String, String>,
}

#[async_trait]
impl<S: SqsStore> TypedAwsAction<S> for CreateQueueAction {
    type Request = CreateQueueRequest;
    type Error = SqsError;

    fn name(&self) -> &'static str { "CreateQueue" }

    async fn handle(
        &self,
        request: ResolvedRequest,
        payload: CreateQueueRequest,
        store: &S,
    ) -> Result<ServiceResponse, SqsError> {
        // Implementation
    }

    async fn resolve_authorization_typed(
        &self,
        request: &ResolvedRequest,
        payload: CreateQueueRequest,
        store: &S,
    ) -> Result<AuthorizationCheck, SqsError> {
        // Return action and resource for authorization
    }
}
```

**Benefits:**
- Each action is a zero-sized type (ZST) with no runtime overhead
- Request parsing is handled automatically by `TypedAwsActionAdapter`
- Type safety for request/response payloads
- Service-specific error types (`SqsError`, `IamError`)
- Consistent error handling across all actions

### 2. Service Registry Pattern

Actions are registered in a type-erased registry that allows runtime dispatch:

```rust
pub struct AwsActionRegistry<S> {
    actions: Vec<Box<dyn AwsAction<S>>>,
}

// In SQS service initialization
pub fn registry<S: SqsStore>() -> AwsActionRegistry<S> {
    let mut registry = AwsActionRegistry::new();
    registry.register_typed(CreateQueueAction);
    registry.register_typed(SendMessageAction);
    registry.register_typed(ReceiveMessageAction);
    // ... more actions
    registry
}
```

The registry dispatches by action name extracted from the AWS request (e.g., `x-amz-target: AmazonSQS.CreateQueue`).

### 3. Store Abstraction Pattern

Storage is abstracted behind async traits, making it easy to swap implementations:

**Trait Definition (hiraeth_store):**

```rust
#[async_trait]
pub trait SqsStore {
    async fn create_queue(&self, queue: SqsQueue) -> Result<(), StoreError>;
    async fn get_queue(&self, name: &str, region: &str, account_id: &str)
        -> Result<Option<SqsQueue>, StoreError>;
    async fn list_queues(&self, region: &str, account_id: &str, prefix: Option<&str>)
        -> Result<Vec<SqsQueue>, StoreError>;
    // ... more methods
}
```

**Production Implementation (hiraeth_store_sqlx):**

```rust
impl SqsStore for SqlxStore {
    async fn create_queue(&self, queue: SqsQueue) -> Result<(), StoreError> {
        sqlx::query!(
            r#"INSERT INTO sqs_queues (name, region, account_id, ...)
               VALUES (?, ?, ?, ...)"#,
            queue.name, queue.region, queue.account_id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
```

**Test Implementation (hiraeth_store test_support):**

```rust
#[cfg(feature = "test-support")]
pub mod test_support {
    pub struct SqsTestStore {
        queues: Arc<Mutex<Vec<SqsQueue>>>,
        // ... in-memory HashMap storage
    }
}
```

**Benefits:**
- Clean separation between business logic and storage
- Compile-time SQL checking with SQLx
- Fast in-memory mock for unit tests
- Easy to add alternative backends (e.g., Postgres, DynamoDB)

### 4. Layered Error Flow

Errors flow through distinct layers, converting at boundaries:

```
StoreError            (storage layer)
    ↓ From impl
SqsError / IamError   (service layer)
    ↓ Into<ServiceResponse>
ServiceResponse       (HTTP layer with AWS error format)
```

**Error Type Definitions:**

```rust
// Storage layer
pub enum StoreError {
    NotFound(String),
    Conflict(String),
    StorageFailure(String),
}

// Service layer (SQS)
pub enum SqsError {
    QueueNotFound,
    BadRequest(String),
    BatchEntryIdsNotDistinct,
    InternalError(String),
    // ... AWS-specific errors
}

// Service layer (IAM)
pub enum IamError {
    NoSuchEntity(String),
    EntityAlreadyExists(String),
    BadRequest(String),
    // ... AWS-specific errors
}
```

**Automatic Conversion:**

```rust
impl From<StoreError> for SqsError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::NotFound(_) => SqsError::QueueNotFound,
            StoreError::Conflict(msg) => SqsError::BadRequest(msg),
            StoreError::StorageFailure(msg) => SqsError::InternalError(msg),
        }
    }
}

impl From<SqsError> for ServiceResponse {
    fn from(value: SqsError) -> Self {
        render_aws_json_error(&value)  // JSON format for SQS
    }
}

impl From<IamError> for ServiceResponse {
    fn from(value: IamError) -> Self {
        // XML format for IAM
        let body = xml_body(&IamErrorResponse::from_error(&value))
            .unwrap_or_else(|_| value.to_string().into_bytes());
        ServiceResponse { status_code, headers, body }
    }
}
```

**Error Handling Convention:**

- Use `Result<ServiceResponse, ServiceError>` in action handlers
- Business logic errors: `Err(SqsError::QueueNotFound)`
- Infrastructure failures: Also `Err(SqsError::InternalError(...))`
- The adapter converts all errors to `ServiceResponse`

This eliminates the ambiguity of having both `Ok(error_response)` and `Err(error)`.

### 5. Authorization Pipeline

Authorization is evaluated in three stages:

```
1. Service Resolution     → Determine which service handles the request
2. Authorization Check    → Service resolves what needs authorization
3. Policy Evaluation      → Authorizer evaluates policies and grants/denies
```

**Stage 1: Service Resolution**

```rust
// In ServiceRouter
let service = self.services.iter()
    .find(|s| s.can_handle(&request))
    .ok_or("No service found")?;
```

**Stage 2: Authorization Check Resolution**

```rust
// Each service implements:
async fn resolve_authorization(
    &self,
    request: &ResolvedRequest,
) -> Result<AuthorizationCheck, ServiceResponse> {
    // Determine action and resource
    let action = extract_action_from_request(request)?;
    let resource = extract_resource_from_request(request)?;

    Ok(AuthorizationCheck {
        action: "sqs:SendMessage",
        resource: "arn:aws:sqs:us-east-1:000000000000:myqueue",
        resource_policy: Some(queue_policy_json),
    })
}
```

**Stage 3: Policy Evaluation**

```rust
// In Authorizer
pub async fn authorize(
    &self,
    request: &ResolvedRequest,
    check: &AuthorizationCheck,
) -> AuthorizationResult {
    // Evaluate resource policy with wildcards
    if policy_allows(&check.action, &check.resource, &check.resource_policy) {
        AuthorizationResult::Allow
    } else {
        AuthorizationResult::Deny
    }
}
```

**Benefits:**
- **What** needs authorization is service-specific
- **How** to authorize is pluggable (different authorizer implementations)
- **When** to enforce is controlled by `AuthMode` (audit/enforce/off)

## Request Lifecycle

```
1. HTTP Request arrives
   ↓
2. Authentication (hiraeth_auth)
   - Extract AWS signature headers
   - Validate SigV4 signature
   - Resolve principal from access key
   ↓
3. Request Resolution
   - Create ResolvedRequest with auth context
   ↓
4. Service Routing (hiraeth_router)
   - Find service that can handle request
   ↓
5. Authorization Resolution
   - Service determines action + resource
   - Extract resource policy if applicable
   ↓
6. Authorization Evaluation
   - Authorizer evaluates policies
   - Deny if mode=enforce and policy denies
   ↓
7. Action Dispatch
   - Registry finds action by name
   - TypedAwsActionAdapter parses payload
   ↓
8. Action Execution
   - Validate request
   - Call store methods
   - Build response
   ↓
9. Response Serialization
   - Convert to JSON/XML
   - Add appropriate headers
   ↓
10. HTTP Response returned
```

## Request Type Progression

Requests are progressively refined through the pipeline:

```rust
// Stage 1: Raw HTTP
IncomingRequest {
    host: String,
    method: String,
    path: String,
    query: Option<String>,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

// Stage 2: After authentication
ResolvedRequest {
    request: IncomingRequest,
    service: String,         // "sqs" or "iam"
    region: String,          // "us-east-1"
    auth_context: AuthContext {
        access_key: String,
        principal: Principal,
    },
    date: DateTime<Utc>,
}

// Stage 3: Action-specific typed request (in handler)
CreateQueueRequest {
    queue_name: String,
    attributes: HashMap<String, String>,
    tags: HashMap<String, String>,
}
```

Each transformation adds structure and validates more constraints.

## Protocol Handling

Different AWS services use different protocols. Hiraeth supports both:

### AWS Query Protocol (IAM)

Form-urlencoded parameters in request body:

```
Action=CreateUser&UserName=test&Path=/
```

Parsed with `parse_aws_query_request()` into typed structs.

### AWS JSON Protocol (SQS)

JSON body with action in header:

```
Headers: x-amz-target: AmazonSQS.CreateQueue
Body: {"QueueName":"test-queue","Attributes":{}}
```

Parsed with `parse_json_body()` into typed structs.

### Response Formats

**JSON (SQS):**
```json
{
  "__type": "com.amazonaws.sqs#QueueDoesNotExist",
  "message": "The specified queue does not exist."
}
```

**XML (IAM):**
```xml
<ErrorResponse xmlns="https://iam.amazonaws.com/doc/2010-05-08/">
  <Error>
    <Type>Sender</Type>
    <Code>NoSuchEntity</Code>
    <Message>User test does not exist</Message>
  </Error>
  <RequestId>uuid</RequestId>
</ErrorResponse>
```

## Testing Strategy

### Unit Tests

**Test Store Implementation:**
```rust
#[cfg(feature = "test-support")]
pub struct SqsTestStore {
    queues: Arc<Mutex<Vec<SqsQueue>>>,
    messages: Arc<Mutex<Vec<SqsMessage>>>,
    // ... in-memory storage
}
```

**Action Tests:**
```rust
#[tokio::test]
async fn create_queue_persists_supplied_tags() {
    let store = SqsTestStore::default();
    let request = resolved_request(r#"{"QueueName":"orders","Tags":{...}}"#);

    let response = handle_create_queue_typed(&request, &store, payload)
        .await
        .expect("create queue should succeed");

    assert_eq!(response.status_code, 200);
    assert_eq!(store.queue_tags(0), expected_tags);
}
```

### Integration Tests

**Real AWS SDK Clients:**
```rust
#[tokio::test]
async fn test_send_receive_with_aws_sdk() {
    let (runtime, endpoint) = setup_test_runtime().await;

    let config = aws_config::from_env()
        .endpoint_url(endpoint)  // Point at Hiraeth
        .load()
        .await;

    let client = aws_sdk_sqs::Client::new(&config);

    // Use real SDK methods
    let result = client.send_message()
        .queue_url(queue_url)
        .message_body("test")
        .send()
        .await;

    assert!(result.is_ok());
}
```

This validates AWS SDK compatibility end-to-end.

### SQL Query Validation

SQLx provides compile-time query checking:

```rust
sqlx::query!(
    r#"INSERT INTO sqs_queues (name, region) VALUES (?, ?)"#,
    queue.name, queue.region
)
```

The `sqlx::query!` macro validates:
- Table and column names exist
- Parameter types match
- Return types match expectations

Metadata is cached in `.sqlx/` and checked in CI.

## Configuration

Environment-driven configuration with sensible defaults:

```rust
#[derive(Deserialize)]
pub struct Config {
    #[serde(default = "default_host")]
    pub host: String,           // HIRAETH_HOST

    #[serde(default = "default_port")]
    pub port: u16,              // HIRAETH_PORT

    pub database_url: String,   // HIRAETH_DATABASE_URL

    #[serde(default)]
    pub auth_mode: AuthMode,    // HIRAETH_AUTH_MODE
}
```

All config uses the `HIRAETH_*` prefix and follows 12-factor app principles.

## Web UI Architecture

The admin UI uses **server-driven rendering** with HTMX:

```html
<!-- Template returns HTML fragments -->
<div hx-get="/sqs/queues" hx-trigger="every 5s">
    {% for queue in queues %}
        <tr>
            <td>{{ queue.name }}</td>
            <td>{{ queue.message_count }}</td>
        </tr>
    {% endfor %}
</div>
```

**Handler:**
```rust
async fn queue_list_fragment(
    State(state): State<Arc<WebState>>,
) -> Result<Html<String>, WebError> {
    let queues = state.stores.sqs_store.list_all_queues().await?;
    let template = QueueListTemplate { queues };
    Ok(Html(template.render()?))
}
```

**Benefits:**
- No JavaScript build step
- Progressive enhancement
- Simple state management (server-side only)
- Fast development iteration

## Performance Considerations

### Zero-Cost Abstractions

- `TypedAwsAction` implementations are zero-sized types (ZSTs)
- Minimal allocations in hot paths

### Connection Pooling

SQLx provides connection pooling automatically:

```rust
let pool = SqlitePoolOptions::new()
    .max_connections(5)
    .connect(&database_url)
    .await?;
```

### Query Optimization

- Indexed columns for common lookups (queue name, region, account_id)
- Prepared statement caching via SQLx
- Efficient visibility timeout queries with indexed `visible_at` column

## Design Principles

1. **Explicit over Implicit** - Request resolution adds explicit context at each stage
2. **Type Safety** - Use the type system to prevent errors at compile time
3. **Single Responsibility** - Each crate has one clear purpose
4. **Testability** - Traits enable easy mocking and testing
5. **Fail Fast** - Validate early, error at construction time when possible
6. **AWS Compatibility** - Match AWS behavior and error formats closely

## Common Patterns

### Adding a New AWS Action

1. Define request/response structs with serde
2. Implement `TypedAwsAction<S>` trait
3. Register action in service registry
4. Add unit tests with `SqsTestStore`
5. Add integration test with AWS SDK client

### Adding a New Service

1. Define store trait in `hiraeth_store`
2. Implement store in `hiraeth_store_sqlx` with migrations
3. Create service crate (e.g., `hiraeth_s3`)
4. Implement `Service` trait with action registry
5. Register service in `hiraeth_router`
6. Add web UI endpoints if needed

### Error Handling

- Storage errors: Return `StoreError` from store methods
- Business errors: Return `ServiceError` (e.g., `SqsError`) from actions
- Let `From` implementations handle conversion
- Use `?` operator for clean propagation

## References

- AWS API Documentation: https://docs.aws.amazon.com/
- AWS SigV4 Signing: https://docs.aws.amazon.com/general/latest/gr/signature-version-4.html
- SQLx Documentation: https://github.com/launchbadge/sqlx
- HTMX Documentation: https://htmx.org/
