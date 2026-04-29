# Hiraeth

Hiraeth is a local AWS emulator focused on fast integration testing. Signed AWS
SDK requests go through a local HTTP endpoint, state is stored in SQLite, and an
optional web UI exposes local service state for debugging.

![Hiraeth web UI showing the SQS dashboard](docs/assets/hiraeth-web-ui.png)

This project is early. It is intended for local development and test
environments, not as a production AWS replacement.

## Current Scope

- AWS SigV4 header authentication with a seeded local test credential.
- Authorization modes for `audit`, `enforce`, and `off`.
- SQS queue resource policy authorization and IAM identity policy evaluation.
- SQLite-backed IAM users, access keys, inline policies, managed policies, SQS
  queues, messages, attributes, and tags.
- SQS-compatible endpoint for common queue and message operations.
- Partial IAM Query API support for users, access keys, inline user policies,
  managed policies, and policy attachments.
- STS `GetCallerIdentity` support.
- SQLite-backed request tracing with span flow visualization in the web UI.
- Web admin UI on a separate port for inspecting local service state.
- Docker and Docker Compose support.
- SQLx offline query metadata for checked SQL builds.

## Documentation

- [Service docs](docs/README.md)
- [IAM](docs/iam/README.md)
- [SQS](docs/sqs/README.md)
- [Tracing](docs/tracing/README.md)

## Quickstart

Start Hiraeth with Docker Compose:

```sh
docker compose up --build
```

The AWS-compatible endpoint listens on `http://localhost:4566`. The admin UI
listens on `http://localhost:4567`.

The default seeded credential is:

```sh
export AWS_ACCESS_KEY_ID=test
export AWS_SECRET_ACCESS_KEY=test
export AWS_DEFAULT_REGION=us-east-1
```

For service-specific examples, API support, and current gaps, see:

- [IAM docs](docs/iam/README.md)
- [SQS docs](docs/sqs/README.md)

Compose writes SQLite data to `/data/db.sqlite` inside the container. Mount your
own volume or bind mount at `/data` if you want data to survive container
recreation.

## Container Image

Release images are published to GitHub Container Registry:

```sh
docker pull ghcr.io/sethpyle376/hiraeth:v0.2.0
```

Release maintainers can publish a multi-architecture image for `linux/amd64`
and `linux/arm64` from a local Docker Buildx environment:

```sh
docker login ghcr.io
scripts/publish-image.sh v0.2.0
```

The publish script pushes `ghcr.io/sethpyle376/hiraeth:<tag>`. Tags must match
the release format `v*.*.*`.

## Running From Source

```sh
mkdir -p .local
HIRAETH_DATABASE_URL=sqlite://.local/db.sqlite cargo run -p hiraeth_runtime
```

Defaults:

| Setting | Environment variable | Default |
| --- | --- | --- |
| AWS emulator host | `HIRAETH_HOST` | `0.0.0.0` |
| AWS emulator port | `HIRAETH_PORT` | `4566` |
| SQLite URL | `HIRAETH_DATABASE_URL` | `sqlite://data/db.sqlite` |
| Authorization mode | `HIRAETH_AUTH_MODE` | `audit` |
| Web UI enabled | `HIRAETH_WEB_ENABLED` | `true` |
| Web UI host | `HIRAETH_WEB_HOST` | `127.0.0.1` |
| Web UI port | `HIRAETH_WEB_PORT` | `4567` |

When running from source, prefer setting `HIRAETH_DATABASE_URL` to a path under
`.local/` or another directory that already exists.

## Web UI

The web UI is an admin/debug surface for local service state. Current
service-specific UI coverage is documented under [docs/iam](docs/iam/README.md)
and [docs/sqs](docs/sqs/README.md).

The tracing dashboard records local AWS requests, spans through the runtime and
service layers, and full request/response bodies. See the
[tracing docs](docs/tracing/README.md) for usage notes and data exposure
details.

The web UI does not use SigV4 authentication. Keep `HIRAETH_WEB_HOST` bound to a
trusted interface unless you intentionally want to expose local test state.

The current UI vendors its JavaScript and CSS assets and serves them from the
Hiraeth web process.

## AI Usage

AI tools are used as part of this project's development workflow for code
generation, refactoring, test writing, documentation drafts, and design
discussion.

Most runtime code has been written by hand, most test code has been generated.
Regardless, all changes are reviewed, edited, and accepted by a human maintainer,
and the project relies on normal engineering checks such as tests, SQLx query
checking, and manual review rather than treating AI output as authoritative.

## Contributing

Compatibility reports, focused bug fixes, docs improvements, and small SQS
parity improvements are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) for the
development workflow, suggested issue labels, and starter contribution ideas.

## License

Hiraeth is licensed under the [MIT License](LICENSE).

## Development

Format and test:

```sh
cargo fmt
cargo test
```

Prepare the local database used by SQLx query checking:

```sh
cargo run -p xtask -- prepare-db
```

Seed SQS queues and messages into a running Hiraeth endpoint:

```sh
cargo run -p xtask -- seed
```

By default this targets `http://localhost:4566` with the local `test`/`test`
credential. Use `cargo run -p xtask -- seed --help` to see endpoint, region,
credential, prefix, and reset options.

Refresh SQLx offline metadata:

```sh
DATABASE_URL=sqlite://.local/db.sqlite cargo sqlx prepare --workspace -- --all-targets
```

Check SQLx metadata in CI-style mode:

```sh
DATABASE_URL=sqlite://.local/db.sqlite cargo sqlx prepare --workspace --check -- --all-targets
```

The checked SQL metadata under `.sqlx/` should be committed when queries or
migrations change.
