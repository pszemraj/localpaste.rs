#!/usr/bin/env python3
"""Shared version normalization helpers for GUI packaging/release scripts."""

from __future__ import annotations

import re

# SemVer 2.0 / Cargo package version pattern, including optional prerelease
# and build metadata segments.
SEMVER_VERSION_RE = re.compile(
    r"^(?P<major>0|[1-9]\d*)\."
    r"(?P<minor>0|[1-9]\d*)\."
    r"(?P<patch>0|[1-9]\d*)"
    r"(?:-(?P<prerelease>"
    r"(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*)"
    r"(?:\.(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*))*"
    r"))?"
    r"(?:\+(?P<build>[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$"
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


def normalize_packaging_version(raw_tag: str) -> str:
    """Normalize stable/prerelease semver into a validated bare version string."""

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

    return normalized_version


def normalize_packaging_tag(raw_tag: str) -> str:
    """Normalize stable/prerelease semver into the repo's `vX.Y.Z` tag form."""

    return f"v{normalize_packaging_version(raw_tag)}"


def packager_version_for_runner(raw_tag: str, *, runner_os: str) -> str:
    """Return the packager config version string for a specific runner OS.

    Windows MSI packaging routes through WiX, whose ProductVersion field accepts
    numeric dotted components only. Verification jobs may derive prerelease
    semver from the workspace version, so Windows needs the numeric semver core
    while artifact naming continues to use the full prerelease tag elsewhere.
    """

    normalized_version = normalize_packaging_version(raw_tag)
    if runner_os.strip() != "Windows":
        return normalized_version

    match = SEMVER_VERSION_RE.fullmatch(normalized_version)
    if match is None:
        raise VersionValidationError(
            "validated semver failed to reparse while deriving Windows packager version"
        )

    return ".".join(
        (
            match.group("major"),
            match.group("minor"),
            match.group("patch"),
        )
    )
