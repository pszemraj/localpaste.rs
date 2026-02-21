#!/usr/bin/env python3
"""Collect and rename GUI release assets from cargo-packager outputs."""

from __future__ import annotations

import argparse
import json
import shutil
import sys
import tarfile
import zipfile
from pathlib import Path


def fail(message: str) -> None:
    print(message, file=sys.stderr)
    raise SystemExit(1)


def find_first(root: Path, pattern: str, *, directories: bool = False) -> Path:
    if not root.exists():
        fail(f"packager output directory does not exist: {root}")

    matches: list[Path] = []
    for candidate in sorted(root.rglob(pattern)):
        if directories and candidate.is_dir():
            matches.append(candidate)
        if not directories and candidate.is_file():
            matches.append(candidate)

    if not matches:
        entry_type = "directory" if directories else "file"
        fail(f"failed to find {entry_type} matching '{pattern}' under {root}")
    return matches[0]


def ensure_non_empty(path: Path) -> None:
    if not path.is_file():
        fail(f"expected file is missing: {path}")
    if path.stat().st_size <= 0:
        fail(f"expected file is empty: {path}")


def collect_windows(
    tag: str,
    asset_suffix: str,
    packager_out: Path,
    release_dir: Path,
    stage_dir: Path,
) -> list[str]:
    msi_source = find_first(packager_out, "*.msi")
    msi_target = release_dir / f"localpaste-{tag}-{asset_suffix}.msi"
    shutil.copy2(msi_source, msi_target)

    stage_binary = stage_dir / "localpaste.exe"
    stage_license = stage_dir / "LICENSE"
    ensure_non_empty(stage_binary)
    ensure_non_empty(stage_license)

    zip_path = release_dir / f"localpaste-{tag}-{asset_suffix}.zip"
    with zipfile.ZipFile(zip_path, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        archive.write(stage_binary, arcname="localpaste.exe")
        archive.write(stage_license, arcname="LICENSE")

    ensure_non_empty(msi_target)
    ensure_non_empty(zip_path)
    return [msi_target.name, zip_path.name]


def collect_linux(
    tag: str,
    asset_suffix: str,
    packager_out: Path,
    release_dir: Path,
    stage_dir: Path,
) -> list[str]:
    appimage_source = find_first(packager_out, "*.AppImage")
    appimage_target = release_dir / f"localpaste-{tag}-{asset_suffix}.AppImage"
    shutil.copy2(appimage_source, appimage_target)

    stage_binary = stage_dir / "localpaste"
    stage_license = stage_dir / "LICENSE"
    ensure_non_empty(stage_binary)
    ensure_non_empty(stage_license)

    tar_path = release_dir / f"localpaste-{tag}-{asset_suffix}.tar.gz"
    with tarfile.open(tar_path, "w:gz") as archive:
        archive.add(stage_binary, arcname="localpaste")
        archive.add(stage_license, arcname="LICENSE")

    ensure_non_empty(appimage_target)
    ensure_non_empty(tar_path)
    return [appimage_target.name, tar_path.name]


def collect_macos(tag: str, asset_suffix: str, packager_out: Path, release_dir: Path) -> list[str]:
    dmg_source = find_first(packager_out, "*.dmg")
    app_bundle = find_first(packager_out, "*.app", directories=True)

    dmg_target = release_dir / f"localpaste-{tag}-{asset_suffix}.dmg"
    shutil.copy2(dmg_source, dmg_target)

    app_tar = release_dir / f"localpaste-{tag}-{asset_suffix}.app.tar.gz"
    with tarfile.open(app_tar, "w:gz") as archive:
        archive.add(app_bundle, arcname=app_bundle.name)

    ensure_non_empty(dmg_target)
    ensure_non_empty(app_tar)
    return [dmg_target.name, app_tar.name]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--asset-suffix", required=True)
    parser.add_argument("--runner-os", required=True)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    tag = args.tag.strip()
    if not tag:
        fail("release tag cannot be empty")
    if not tag.startswith("v"):
        fail(f"release tag must start with 'v' (got: {tag})")

    runner_os = args.runner_os.strip()
    asset_suffix = args.asset_suffix.strip()

    packager_out = Path("dist") / "packager" / asset_suffix
    release_dir = Path("dist") / "release" / asset_suffix
    stage_dir = Path("dist") / "stage" / asset_suffix
    release_dir.mkdir(parents=True, exist_ok=True)

    artifacts: list[str]
    if runner_os == "Windows":
        artifacts = collect_windows(tag, asset_suffix, packager_out, release_dir, stage_dir)
    elif runner_os == "Linux":
        artifacts = collect_linux(tag, asset_suffix, packager_out, release_dir, stage_dir)
    elif runner_os == "macOS":
        artifacts = collect_macos(tag, asset_suffix, packager_out, release_dir)
    else:
        fail(f"unsupported runner OS: {runner_os}")

    manifest = {
        "tag": tag,
        "asset_suffix": asset_suffix,
        "runner_os": runner_os,
        "artifacts": artifacts,
    }
    (release_dir / "manifest.json").write_text(
        json.dumps(manifest, indent=2) + "\n",
        encoding="utf-8",
    )

    print(f"collected release assets under {release_dir}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
