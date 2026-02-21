#!/usr/bin/env bash
set -euo pipefail

# Print [workspace.package].version from Cargo.toml.
#
# Usage:
#   .github/scripts/workspace_version.sh [path/to/Cargo.toml]
#
# Exits non-zero if the workspace version cannot be resolved.

CARGO_TOML_PATH="${1:-Cargo.toml}"

WORKSPACE_VERSION="$(
  awk '
    $0 == "[workspace.package]" { in_workspace = 1; next }
    /^\[/ { if (in_workspace) exit }
    in_workspace && $1 == "version" {
      gsub(/"/, "", $3)
      print $3
      exit
    }
  ' "${CARGO_TOML_PATH}"
)"

if [[ -z "${WORKSPACE_VERSION}" ]]; then
  echo "failed to read [workspace.package].version from ${CARGO_TOML_PATH}" >&2
  exit 1
fi

printf '%s\n' "${WORKSPACE_VERSION}"
