# GUI Release Pipeline

This document is the canonical release contract for GUI packaging and publication.

Primary implementation:

- [`.github/workflows/release-gui.yml`](../.github/workflows/release-gui.yml)
- [`.github/workflows/verify-gui-packaging.yml`](../.github/workflows/verify-gui-packaging.yml)
- [`.github/scripts/release_gui_prepare.py`](../.github/scripts/release_gui_prepare.py)
- [`.github/scripts/release_gui_collect.py`](../.github/scripts/release_gui_collect.py)

## Modes

`release-gui.yml` supports two source modes:

- `release_tag`: package from an existing `v*` tag and publish assets.
- `current_ref`: package from the current commit for verification; publish job is skipped.

`release_tag` gates:

- tag format/existence validation,
- workspace version == tag version,
- server+CLI smoke test including restart persistence,
- packaging/build jobs check out the resolved source ref directly (full-tree tag fidelity in `release_tag` mode).

## Artifact Contract

Published release assets (when produced) follow:

- `localpaste-<tag>-windows-x86_64.msi`
- `localpaste-<tag>-windows-x86_64.zip`
- `localpaste-<tag>-linux-x86_64.AppImage`
- `localpaste-<tag>-linux-x86_64.tar.gz`
- `localpaste-<tag>-macos-aarch64.dmg`
- `localpaste-<tag>-macos-aarch64.app.tar.gz`
- `checksums.sha256`

Windows and Linux artifacts are always expected for successful release runs.

Packaging verification checks include:

- Windows: MSI presence + non-empty payload + administrative extraction contains `localpaste.exe`.
- Linux: AppImage presence + non-empty payload + runtime metadata check via `--appimage-version`.
- macOS: DMG integrity/format validation, plus signed-bundle verification inside mounted DMG when notarization secrets are present.

## CI Integrity Controls

Release/packaging workflows enforce these baseline controls:

- Least privilege by default: workflow-level `permissions: contents: read`, with publish-only elevation to `contents: write`.
- Immutable action pinning (`uses:` entries pinned to commit SHAs) for release-critical jobs.
- Deterministic source checkout for packaging jobs via resolved `SOURCE_REF` (no selective tree overlay from a different ref).
- Windows WiX toolchain pinning (`3.14.1`) plus major-version assertion in `release_gui_prepare.py`.

These controls are part of the release contract and should be preserved when editing release workflows.

## macOS Signing And Notarization

Signing/notarization runs only when Apple secrets are present
(`APPLE_SIGNING_*`, `APPLE_ID`, `APPLE_APP_SPECIFIC_PASSWORD`, `APPLE_TEAM_ID`).

Behavior when secrets are missing:

- `release_tag`: macOS artifacts are still built/published in permissive mode as unsigned/unnotarized.
- `current_ref`: unsigned macOS packaging build is allowed for verification runs.

Behavior when secrets are present:

- `release_tag` and `current_ref`: workflow signs, notarizes, and staples macOS artifacts.

## Release Notes Gatekeeper Note

When a `.dmg` is present in published assets, the workflow appends this one-line macOS note to the release body (idempotent):

`macOS note: this release may include unsigned/unnotarized LocalPaste macOS artifacts. If Gatekeeper blocks LocalPaste, use Open Anyway in System Settings > Privacy & Security or run \`xattr -cr /Applications/LocalPaste.app\`.`
