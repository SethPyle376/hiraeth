# Contributing

Thanks for taking a look at Hiraeth. The project is early, so the most useful
contributions are small, focused changes that improve AWS client compatibility,
test coverage, documentation, or local debugging ergonomics.

## Good Contribution Areas

- Report AWS SDK compatibility gaps with a small reproduction.
- Add integration tests that exercise real AWS SDK clients against Hiraeth.
- Tighten SQS request validation and AWS-style error responses.
- Improve documentation for local development, SDK setup, Docker, or config.
- Make small web UI improvements that help inspect local emulator state.

Large design changes, new services, or IAM/policy work are worth discussing in
an issue before opening a large pull request.

## Development Setup

Install the Rust toolchain used by CI:

```sh
rustup toolchain install 1.95.0
```

Run the main checks:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test
```

Prepare the local SQLite database used by SQLx query checking:

```sh
cargo run -p xtask -- prepare-db
DATABASE_URL=sqlite://.local/db.sqlite cargo sqlx prepare --workspace --check -- --all-targets
```

Run the emulator locally:

```sh
docker compose up --build
```

The AWS-compatible endpoint defaults to `http://localhost:4566` and the web UI
defaults to `http://localhost:4567`.

## Compatibility Reports

When reporting a client compatibility issue, include as much of this as you can:

- The AWS client or tool name and version.
- The Hiraeth version or image tag.
- The SQS operation that failed.
- The expected AWS behavior.
- The actual Hiraeth response, error code, or client exception.
- A minimal reproduction using the AWS CLI or an SDK snippet.

Real client failures are especially useful when they can become integration
tests.

## Suggested Labels

Useful labels for triage:

- `good first issue`: small, well-scoped work for new contributors.
- `aws-parity`: behavior differs from AWS or needs compatibility validation.
- `sqs`: SQS API, storage, validation, or response behavior.
- `iam`: future authorization, principal, policy, and identity work.
- `web-ui`: admin/debug UI work.
- `tests`: unit or integration test coverage.
- `docs`: README, examples, release notes, or contributor documentation.
- `bug`: incorrect behavior or client-visible failure.
- `enhancement`: additive functionality or ergonomics improvement.

The repo includes `.github/labels.yml` as a label manifest. GitHub does not
sync this file automatically; it is intended as a source of truth for manual
setup or a future label-sync workflow.

## Starter Issue Ideas

- Add an AWS SDK integration test for one SQS edge case that is not covered yet.
- Improve validation for one SQS API request and add matching error tests.
- Add a README snippet for configuring another AWS SDK against Hiraeth.
- Document a known AWS parity gap with a concrete example.
- Add a small web UI convenience action that does not change emulator behavior.

## Pull Requests

Keep pull requests focused. A good PR usually changes one behavior, adds or
updates tests for that behavior, and includes a short explanation of the client
or developer workflow it improves.
