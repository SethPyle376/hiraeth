#!/usr/bin/env bash
set -euo pipefail

IMAGE="${IMAGE:-ghcr.io/sethpyle376/hiraeth}"
TAG="${1:-${TAG:-}}"
BUILDER="${BUILDER:-hiraeth-release-builder}"

if [[ -z "${TAG}" ]]; then
  TAG="$(git describe --tags --exact-match 2>/dev/null || true)"
fi

if [[ -z "${TAG}" ]]; then
  echo "usage: scripts/publish-image.sh <tag>" >&2
  echo "example: scripts/publish-image.sh v0.1.0" >&2
  echo "or set TAG=v0.1.0" >&2
  exit 2
fi

if [[ ! "${TAG}" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "release tag must look like v0.1.0; got '${TAG}'" >&2
  exit 2
fi

docker buildx inspect "${BUILDER}" >/dev/null 2>&1 || docker buildx create --name "${BUILDER}" --use >/dev/null
docker buildx use "${BUILDER}"

docker buildx build \
  --platform linux/amd64,linux/arm64 \
  --tag "${IMAGE}:${TAG}" \
  --push \
  .
