# GUI Release Pipeline

This document is the canonical release contract for GUI packaging and publication.

Primary implementation: [`.github/workflows/release-gui.yml`](../.github/workflows/release-gui.yml), [`.github/scripts/release_gui_prepare.py`](../.github/scripts/release_gui_prepare.py), [`.github/scripts/release_gui_collect.py`](../.github/scripts/release_gui_collect.py).

## Modes

`release-gui.yml` supports two source modes:

- `release_tag`: package from an existing `v*` tag and publish assets.
- `current_ref`: package from the current commit for verification; publish job is skipped.

`release_tag` gates:

- tag format/existence validation,
- workspace version == tag version,
- server+CLI smoke test including restart persistence.

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

## macOS Signing And Notarization

`release_tag` mode requires Apple signing/notarization secrets (`APPLE_SIGNING_*`, `APPLE_ID`, `APPLE_APP_SPECIFIC_PASSWORD`, `APPLE_TEAM_ID`) for macOS artifact publication.

Behavior when secrets are missing:

- `release_tag`: macOS artifact build is skipped; release continues for other platforms.
- `current_ref`: unsigned macOS packaging build is allowed for verification runs.

## Release Notes Gatekeeper Note

When a `.dmg` is present in published assets, the workflow appends this one-line macOS note to the release body (idempotent):

`macOS note: if Gatekeeper blocks LocalPaste, use Open Anyway in System Settings > Privacy & Security or run \`xattr -cr /Applications/LocalPaste.app\`.`
