#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: normalize_release_tag.sh <tag-or-version>" >&2
  exit 1
fi

RAW_TAG="$1"
TRIMMED_TAG="$(printf '%s' "${RAW_TAG}" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')"

if [[ -z "${TRIMMED_TAG}" ]]; then
  echo "release tag cannot be empty" >&2
  exit 1
fi

NORMALIZED_VERSION="${TRIMMED_TAG}"
NORMALIZED_VERSION="${NORMALIZED_VERSION#v}"
NORMALIZED_VERSION="${NORMALIZED_VERSION#V}"

if [[ ! "${NORMALIZED_VERSION}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "release tag/version must match stable vX.Y.Z or X.Y.Z format (got: ${TRIMMED_TAG})" >&2
  exit 1
fi

printf 'v%s\n' "${NORMALIZED_VERSION}"
