#!/usr/bin/env python3
"""Normalize packaging versions for verification workflows."""

from __future__ import annotations

import sys

from release_versioning import VersionValidationError, normalize_packaging_tag


def main(argv: list[str] | None = None) -> int:
    args = argv if argv is not None else sys.argv[1:]
    if len(args) != 1:
        print("usage: normalize_packaging_tag.py <tag-or-version>", file=sys.stderr)
        return 1

    try:
        normalized = normalize_packaging_tag(args[0])
    except VersionValidationError as exc:
        print(str(exc), file=sys.stderr)
        return 1

    print(normalized)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
