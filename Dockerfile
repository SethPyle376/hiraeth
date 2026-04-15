# syntax=docker/dockerfile:1

FROM rust:1-slim-bookworm AS builder

ARG TARGETARCH

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential \
        ca-certificates \
        musl-tools \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

RUN case "$TARGETARCH" in \
        amd64) echo "x86_64-unknown-linux-musl" > /tmp/rust-target ;; \
        arm64) echo "aarch64-unknown-linux-musl" > /tmp/rust-target ;; \
        *) echo "unsupported target architecture: $TARGETARCH" >&2; exit 1 ;; \
    esac \
    && rustup target add "$(cat /tmp/rust-target)"

WORKDIR /workspace

# SQLx macros should use the checked-in offline query metadata during image builds.
ENV SQLX_OFFLINE=true \
    CC_aarch64_unknown_linux_musl=musl-gcc \
    CC_x86_64_unknown_linux_musl=musl-gcc \
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=musl-gcc \
    CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=musl-gcc

COPY . .

RUN cargo build --locked --release --target "$(cat /tmp/rust-target)" -p hiraeth_runtime \
    && cp "target/$(cat /tmp/rust-target)/release/hiraeth_runtime" /tmp/hiraeth
RUN mkdir -p /tmp/hiraeth-data && touch /tmp/hiraeth-data/.keep

FROM gcr.io/distroless/static-debian12:nonroot

WORKDIR /

COPY --from=builder --chown=nonroot:nonroot /tmp/hiraeth-data/ /data/
COPY --from=builder /tmp/hiraeth /usr/local/bin/hiraeth

ENV HIRAETH_HOST=0.0.0.0 \
    HIRAETH_PORT=4566 \
    HIRAETH_WEB_HOST=0.0.0.0 \
    HIRAETH_WEB_PORT=4567 \
    HIRAETH_DATABASE_URL=sqlite:///data/db.sqlite

EXPOSE 4566 4567

USER nonroot:nonroot
ENTRYPOINT ["/usr/local/bin/hiraeth"]
