#!/usr/bin/env python3
"""Prepare release GUI packager config, staging, and Windows WiX environment.

Design goals:
- Deterministic versioning: derive packager version from the release tag.
- Strict inputs: fail fast with actionable errors.
- Windows resilience: prefer a WiX installation that matches the expected major
  version when multiple WiX installs exist on the runner.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import stat
import subprocess
import sys
from pathlib import Path
from typing import Iterable, Sequence


def fail(message: str) -> None:
    print(message, file=sys.stderr)
    raise SystemExit(1)


def append_github_env(name: str, value: str) -> None:
    env_file = os.environ.get("GITHUB_ENV")
    if not env_file:
        return
    with Path(env_file).open("a", encoding="utf-8", newline="\n") as handle:
        handle.write(f"{name}={value}\n")


def append_github_path(path: Path) -> None:
    path_file = os.environ.get("GITHUB_PATH")
    if not path_file:
        return
    with Path(path_file).open("a", encoding="utf-8", newline="\n") as handle:
        handle.write(f"{path}\n")


def unique_paths(paths: Iterable[Path]) -> list[Path]:
    seen: set[str] = set()
    unique: list[Path] = []
    for path in paths:
        key = str(path.resolve()) if path.exists() else str(path)
        if key in seen:
            continue
        seen.add(key)
        unique.append(path)
    return unique


def wix_candidate_dirs() -> list[Path]:
    candidates: list[Path] = []

    wix_root = os.environ.get("WIX")
    if wix_root:
        candidates.append(Path(wix_root) / "bin")

    for env_key in ("ProgramFiles(x86)", "ProgramFiles"):
        base = os.environ.get(env_key)
        if not base:
            continue
        base_path = Path(base)
        if not base_path.exists():
            continue
        for child in sorted(base_path.iterdir()):
            if child.is_dir() and child.name.lower().startswith("wix"):
                candidates.append(child / "bin")

    for executable in ("candle.exe", "light.exe"):
        discovered = shutil.which(executable)
        if discovered:
            candidates.append(Path(discovered).parent)

    return unique_paths(candidates)


def probe_wix_version(wix_bin: Path) -> str | None:
    """Return WiX version string if candle/light can be executed, else None."""

    version_pattern = re.compile(r"version\s+([0-9]+(?:\.[0-9]+){1,3})", re.IGNORECASE)

    detected_version: str | None = None
    for executable in ("candle.exe", "light.exe"):
        command = wix_bin / executable
        if not command.is_file():
            return None
        result = subprocess.run(
            [str(command), "-?"],
            capture_output=True,
            text=True,
            check=False,
        )
        if result.returncode != 0:
            return None
        if detected_version is None:
            output = f"{result.stdout}\n{result.stderr}"
            match = version_pattern.search(output)
            if match:
                detected_version = match.group(1)

    return detected_version


def wix_major(version: str) -> int | None:
    major_raw = version.split(".", 1)[0]
    try:
        return int(major_raw)
    except ValueError:
        return None


def discover_wix_bin(expected_major: int) -> tuple[Path, str]:
    """Locate a WiX bin directory that matches expected_major.

    Runners can end up with multiple WiX installs (preinstalled + Chocolatey,
    or WiX 3 + WiX 4). We should select the one that matches the expected major
    version instead of taking the first match.
    """

    candidates = wix_candidate_dirs()

    valid: list[tuple[Path, str]] = []
    checked: list[Path] = []

    for candidate in candidates:
        checked.append(candidate)
        version = probe_wix_version(candidate)
        if not version:
            continue
        valid.append((candidate, version))
        major = wix_major(version)
        if major == expected_major:
            return candidate, version

    if valid:
        joined = "\n".join(
            f"- {path} (version {version})" for path, version in valid
        )
        fail(
            "found WiX installations, but none match the expected major version\n"
            f"expected_major={expected_major}\n"
            f"detected:\n{joined}"
        )

    if checked:
        joined = "\n".join(f"- {entry}" for entry in checked)
        fail(
            "failed to locate a usable WiX bin directory containing candle.exe and light.exe\n"
            f"checked candidate directories:\n{joined}"
        )

    fail("failed to locate WiX installation candidates")


def resolve_icon_paths(config_dir: Path, icons: list[str]) -> list[Path]:
    resolved: list[Path] = []
    for icon in icons:
        icon_path = Path(icon)
        if not icon_path.is_absolute():
            icon_path = config_dir / icon_path
        icon_path = icon_path.resolve()
        if not icon_path.is_file():
            fail(f"packager icon path does not exist: {icon_path}")
        resolved.append(icon_path)
    return resolved


def stage_runtime_binary(runner_os: str, target: str, asset_suffix: str) -> None:
    stage_dir = Path("dist") / "stage" / asset_suffix
    stage_dir.mkdir(parents=True, exist_ok=True)

    if runner_os == "Windows":
        source_binary = Path("target") / target / "release" / "localpaste-gui.exe"
        staged_binary = stage_dir / "localpaste.exe"
    else:
        source_binary = Path("target") / target / "release" / "localpaste-gui"
        staged_binary = stage_dir / "localpaste"

    license_src = Path("LICENSE")
    staged_license = stage_dir / "LICENSE"

    if not source_binary.is_file():
        fail(f"runtime binary not found: {source_binary}")
    if not license_src.is_file():
        fail("LICENSE file missing at repository root")

    shutil.copy2(source_binary, staged_binary)
    shutil.copy2(license_src, staged_license)

    if runner_os != "Windows":
        mode = staged_binary.stat().st_mode
        staged_binary.chmod(mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)

    if staged_binary.stat().st_size <= 0:
        fail(f"staged runtime binary is empty: {staged_binary}")
    if staged_license.stat().st_size <= 0:
        fail(f"staged LICENSE is empty: {staged_license}")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--asset-suffix", required=True)
    parser.add_argument("--packager-config", required=True)
    parser.add_argument("--runner-os", required=True)
    parser.add_argument("--expected-wix-major", type=int, default=3)
    return parser.parse_args()


def main() -> int:
    args = parse_args()

    tag = args.tag.strip()
    if not tag:
        fail("release tag cannot be empty")
    if not tag.startswith("v"):
        fail(f"release tag must start with 'v' (got: {tag})")

    version = tag[1:]
    if not version:
        fail(f"release tag is missing semantic version segment: {tag}")

    runner_os = args.runner_os.strip()
    source_config = Path(args.packager_config)
    if not source_config.is_file():
        fail(f"packager config file not found: {source_config}")

    config_dir = source_config.parent
    effective_config = config_dir / "packager.effective.json"

    config = json.loads(source_config.read_text(encoding="utf-8"))
    if not isinstance(config, dict):
        fail(f"packager config must be a JSON object: {source_config}")

    config["version"] = version

    if runner_os == "macOS":
        icns_path = Path("assets/icons/desktop_icon.icns")
        if not icns_path.is_file() or icns_path.stat().st_size <= 0:
            fail("expected generated macOS icon at assets/icons/desktop_icon.icns")
        config["icons"] = ["../../assets/icons/desktop_icon.icns"]

    formats = config.get("formats")
    if not isinstance(formats, list) or not formats:
        fail("packager config must define at least one format")

    if runner_os == "Windows":
        normalized_formats = {str(item).lower() for item in formats}
        if "wix" not in normalized_formats:
            fail("windows packager config must include 'wix' format")

    icons_raw = config.get("icons")
    if not isinstance(icons_raw, list) or not icons_raw:
        fail("packager config must define at least one icon")

    icons = [str(entry) for entry in icons_raw]
    resolved_icons = resolve_icon_paths(config_dir, icons)

    if runner_os == "macOS" and not any(icon.lower().endswith(".icns") for icon in icons):
        fail("macOS packager config must include an .icns icon path")

    if runner_os == "Windows" and not any(icon.suffix.lower() == ".ico" for icon in resolved_icons):
        fail("windows packager config must include at least one .ico icon path")

    if runner_os == "Windows":
        wix_bin, wix_version = discover_wix_bin(args.expected_wix_major)
        wix_root = wix_bin.parent
        append_github_path(wix_bin)
        append_github_env("WIX", str(wix_root))
        append_github_env("WIX_VERSION_DETECTED", wix_version)
        print(f"using WiX: {wix_bin} (version {wix_version})")

    stage_runtime_binary(runner_os=runner_os, target=args.target, asset_suffix=args.asset_suffix)

    effective_config.write_text(json.dumps(config, indent=2) + "\n", encoding="utf-8")
    append_github_env("PACKAGER_CONFIG_PATH", str(effective_config.resolve()))

    print(f"prepared packager config: {effective_config}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
