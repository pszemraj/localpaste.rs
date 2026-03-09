#!/usr/bin/env python3
"""Shared version normalization helpers for GUI packaging/release scripts."""

from __future__ import annotations

import re

# SemVer 2.0 / Cargo package version pattern, including optional prerelease
# and build metadata segments.
SEMVER_VERSION_RE = re.compile(
    r"^(0|[1-9]\d*)\."
    r"(0|[1-9]\d*)\."
    r"(0|[1-9]\d*)"
    r"(?:-"
    r"(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*)"
    r"(?:\.(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*))*"
    r")?"
    r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$"
)


class VersionValidationError(ValueError):
    """Raised when a tag/version string fails normalization."""


def _strip_and_remove_leading_v(raw_tag: str, *, empty_message: str) -> str:
    tag = raw_tag.strip()
    if not tag:
        raise VersionValidationError(empty_message)

    if tag[:1] in {"v", "V"}:
        return tag[1:]
    return tag


def normalize_packaging_tag(raw_tag: str) -> str:
    """Normalize stable/prerelease semver into the repo's `vX.Y.Z` tag form."""

    normalized_version = _strip_and_remove_leading_v(
        raw_tag,
        empty_message="packaging tag/version cannot be empty",
    )

    if not SEMVER_VERSION_RE.fullmatch(normalized_version):
        raise VersionValidationError(
            "packaging tag/version must match Cargo semver "
            "vX.Y.Z[-PRERELEASE][+BUILD] or X.Y.Z[-PRERELEASE][+BUILD] format "
            f"(got: {raw_tag})"
        )

    return f"v{normalized_version}"
