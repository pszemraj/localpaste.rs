"""Locate and validate the application-local Microsoft C++ runtime payload."""

from __future__ import annotations

import os
import re
import subprocess
from pathlib import Path
from typing import Iterator


# Imported by the GUI's statically linked ONNX Runtime C++ code on Windows x64.
REQUIRED_DLLS = ("msvcp140.dll", "msvcp140_1.dll", "vcruntime140.dll", "vcruntime140_1.dll")


def runtime_dlls(directory: Path) -> list[Path]:
    """Return redistributable DLLs, rejecting a missing or empty required DLL."""
    for name in REQUIRED_DLLS:
        path = directory / name
        if not path.is_file() or path.stat().st_size == 0:
            raise ValueError(f"missing or empty Microsoft C++ runtime DLL: {path}")
    return sorted(path for path in directory.iterdir() if path.is_file() and path.suffix.lower() == ".dll")


def vc_redist_roots() -> Iterator[Path]:
    """Try the active toolchain before installed toolchains, newest version first."""
    redist_env = os.environ.get("VCToolsRedistDir")
    if redist_env:
        yield Path(redist_env)

    program_files = os.environ.get("ProgramFiles(x86)")
    if program_files:
        vswhere = Path(program_files) / "Microsoft Visual Studio/Installer/vswhere.exe"
        if vswhere.is_file():
            result = subprocess.run([
                str(vswhere), "-latest", "-products", "*", "-requires",
                "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "installationPath",
            ], check=True, capture_output=True, text=True)
            installation = result.stdout.strip()
            if installation:
                redist = Path(installation) / "VC/Redist/MSVC"
                if redist.is_dir():
                    versions = [path for path in redist.iterdir()
                                if path.is_dir() and re.fullmatch(r"\d+(?:\.\d+)+", path.name)]
                    yield from sorted(versions,
                        key=lambda path: tuple(int(part) for part in path.name.split(".")), reverse=True)


def discover_vc_runtime() -> Path:
    """Find the x64 redistributable shipped with the runner's Visual Studio tools."""
    for root in vc_redist_roots():
        for candidate in sorted(root.glob("x64/Microsoft.VC*.CRT")):
            try:
                runtime_dlls(candidate)
            except ValueError:
                continue
            return candidate
    raise ValueError("failed to locate a complete x64 Microsoft C++ redistributable; "
                     "install the Visual Studio C++ tools or set VCToolsRedistDir")
