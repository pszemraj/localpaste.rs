#!/usr/bin/env python3
"""Prepare release GUI packager config, staging, and Windows WiX environment."""

from __future__ import annotations

import argparse
import json
import os
import shutil
import stat
import subprocess
import sys
from pathlib import Path
from typing import Iterable


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


def discover_wix_bin() -> Path:
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

    checked: list[Path] = []
    for candidate in unique_paths(candidates):
        checked.append(candidate)
        if (candidate / "candle.exe").is_file() and (candidate / "light.exe").is_file():
            return candidate

    if checked:
        joined = "\n".join(f"- {entry}" for entry in checked)
        fail(
            "failed to locate WiX bin directory containing candle.exe and light.exe\n"
            f"checked candidate directories:\n{joined}"
        )

    fail("failed to locate WiX installation candidates")


def validate_wix_tools(wix_bin: Path) -> None:
    for executable in ("candle.exe", "light.exe"):
        command = str(wix_bin / executable)
        result = subprocess.run([command, "-?"], capture_output=True, text=True, check=False)
        if result.returncode != 0:
            fail(f"{command} failed self-check with exit code {result.returncode}")


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
    resolve_icon_paths(config_dir, icons)

    if runner_os == "macOS" and not any(icon.lower().endswith(".icns") for icon in icons):
        fail("macOS packager config must include an .icns icon path")

    if runner_os == "Windows":
        wix_bin = discover_wix_bin()
        validate_wix_tools(wix_bin)
        wix_root = wix_bin.parent
        append_github_path(wix_bin)
        append_github_env("WIX", str(wix_root))

    stage_runtime_binary(runner_os=runner_os, target=args.target, asset_suffix=args.asset_suffix)

    effective_config.write_text(json.dumps(config, indent=2) + "\n", encoding="utf-8")
    append_github_env("PACKAGER_CONFIG_PATH", str(effective_config.resolve()))

    print(f"prepared packager config: {effective_config}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
