# GUI Release Pipeline

This page is for maintainers. It describes how GUI release artifacts are built, verified, and published, and where to change each part. Artifact names and installation steps for users are in [../release-gui.md](../release-gui.md).

## Workflows

[`release-gui.yml`](../../.github/workflows/release-gui.yml) builds and publishes the artifacts. It smoke-tests the server and CLI on Linux (the flow in the [devlog smoke runbook](devlog.md#runtime-smoke-test-server-cli), including restart persistence), then builds, packages, and checks the GUI on Windows, Linux, and macOS in parallel, and finally uploads the assets and a `checksums.sha256` file to the GitHub release for the tag. Publishing needs all three platform jobs to succeed.

[`verify-gui-packaging.yml`](../../.github/workflows/verify-gui-packaging.yml) is a manual, macOS-only rehearsal: workspace check, GUI tests, packaging, and signing when credentials exist. It publishes nothing and keeps its output as a workflow artifact for seven days.

[`workflow-lint.yml`](../../.github/workflows/workflow-lint.yml) runs `validate_workflow.py` (YAML, embedded shell, and release invariants such as job order, the platform matrix, and the macOS signing and WiX rules) plus the release helper tests. It runs on pull requests that touch workflows or scripts, and manually. The same commands are in the [validation loop](devlog.md#validation-loop) for local runs.

## Workflow triggers

`release-gui.yml` runs when a tag matching `vX.Y.Z` is pushed, or manually. The other two workflows are manual or, for lint, pull-request driven. Nothing runs on pushes to the main branch or on a schedule.

## Source modes

A run packages either a release tag or the current commit.

In `release_tag` mode the tag must be a stable `vX.Y.Z` (the leading `v` is optional on manual input) that exists in the repository, and `[workspace.package].version` in the root `Cargo.toml` at that tag must equal it. Every job checks out the tag's commit, so the build matches the tag exactly. A pushed tag always uses this mode and publishes.

In `current_ref` mode the run packages the commit it was started from and never publishes. The version comes from the workspace version, which may be a prerelease. Artifact names keep the full prerelease version, but the MSI uses only `major.minor.patch`, because WiX product versions cannot carry prerelease or build metadata.

A manual run defaults to `current_ref` with `dry_run` enabled. To publish manually, choose `release_tag`, give an existing stable tag, and set `dry_run` to false.

## Cutting a release

1. Set `[workspace.package].version` in the root `Cargo.toml` to the release version and merge it. The workflow rejects a tag that disagrees with it.
2. Optionally rehearse: start `release-gui` manually on the branch with the defaults. The packages are kept as workflow artifacts for one day.
3. Tag the release commit `vX.Y.Z` and push the tag.
4. Watch the run. On success the release for the tag holds the six packages and `checksums.sha256`. The workflow writes no release notes beyond the macOS note below, so add the rest on the release page.

Re-running the workflow for a tag replaces assets that have the same names.

## Packaging

Each platform job builds `localpaste-gui` in release mode and stages it as `localpaste` (`localpaste.exe` on Windows) next to the `LICENSE`. `release_gui_prepare.py` writes the effective packager config from `packaging/<platform>/packager.json` with the release version filled in, and `cargo-packager` produces the MSI, AppImage, or DMG and app bundle from it. `release_gui_collect.py` then names the assets, adds the ZIP or tarball of the staged binary (the app bundle on macOS), and the job uploads them.

Before upload the job checks its package: the MSI must contain `localpaste.exe` after administrative extraction, the AppImage must report its runtime version and extract, and the DMG must pass `hdiutil verify`, plus a signature check of the app inside the DMG when the build is signed.

Windows packaging needs a WiX 3 toolchain. `release_gui_prepare.py` finds an installed one and asserts its major version, and the workflow installs WiX 3.14.1 from Chocolatey when no WiX 3 installation is found. That installation is only a preflight: `cargo-packager` ignores it and downloads its own WiX 3.11.2 into `WixTools` under `<cache-dir>/.cargo-packager`, as the pinned version's [WiX code](https://github.com/crabnebula-dev/cargo-packager/blob/cargo-packager-v0.11.8/crates/packager/src/package/wix/mod.rs) shows.

## macOS signing and notarization

Signing runs only when all of these repository secrets are set: `APPLE_SIGNING_CERT_BASE64`, `APPLE_SIGNING_CERT_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_APP_SPECIFIC_PASSWORD`, and `APPLE_TEAM_ID`. With them, the job signs the app bundle and the DMG, notarizes the DMG, and staples both. Without them, a `release_tag` run still publishes the macOS artifacts unsigned and unnotarized, and a `current_ref` run builds them unsigned.

When a release contains a DMG, the publish job appends a one-line Gatekeeper note to the release body, once. Its wording matches the instructions in [../release-gui.md](../release-gui.md#macos-gatekeeper); keep the two in step.

## Integrity controls

The workflows default to a read-only token, and only the publish job has `contents: write`. Third-party actions are pinned to commit SHAs with the version in a trailing comment; keep that when updating them. The Rust toolchain and `cargo-packager` versions are pinned in the workflow, so changing either is a workflow edit.

## Where to change what

| To change | Edit |
| --- | --- |
| Runners, targets, job order, publishing, pinned tool versions | `.github/workflows/release-gui.yml` |
| Package formats, product name, identifier, minimum macOS version | `packaging/<platform>/packager.json` |
| Version injection, staging, WiX preflight | `.github/scripts/release_gui_prepare.py` |
| Asset names and archive contents | `.github/scripts/release_gui_collect.py` |
| Tag and version rules | `.github/scripts/release_versioning.py`, `normalize_release_tag.sh`, `normalize_packaging_tag.py`, `workspace_version.sh` |
| Invariants enforced on pull requests | `.github/scripts/validate_workflow.py` |
