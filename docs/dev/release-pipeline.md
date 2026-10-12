# GUI Release Pipeline

Artifact names and installation steps for users are in [GUI Releases](../release-gui.md).

## Workflows

[`release-gui.yml`](../../.github/workflows/release-gui.yml) builds and uploads artifacts when a GitHub Release is published or when started manually; the release tag must be a stable `vX.Y.Z`. It runs a server and CLI smoke test on Linux similar to the [devlog runbook](devlog.md#runtime-smoke-test-server-cli) (create, search, delete, and a second paste read back after a restart), then builds, packages, and checks the GUI on Windows, Linux, and macOS in parallel. A run that [publishes](#source-modes) then uploads the assets and a `checksums.sha256` file to the GitHub release for the tag, which needs all three platform jobs to succeed.

[`verify-gui-packaging.yml`](../../.github/workflows/verify-gui-packaging.yml) is a manual, macOS-only rehearsal: workspace check, GUI tests, packaging, and signing when credentials exist. Its Cargo commands use `--locked`, like release builds. It publishes nothing and keeps its output as a workflow artifact for seven days.

[`workflow-lint.yml`](../../.github/workflows/workflow-lint.yml) runs `validate_workflow.py` (YAML, embedded shell, and release invariants such as job order, the platform matrix, and the macOS signing and WiX rules) plus the release helper tests, on pull requests that touch workflows or scripts and when started manually. The same commands are in the [validation loop](devlog.md#validation-loop) for local runs.

Release builds do not run on PR merges, branch or tag pushes, draft saves, release-note edits, or a schedule. Publishing a saved draft triggers the build, just like publishing a new release directly. The workflow validator rejects extra release-build triggers, including schedules and tag pushes.

## Source modes

A run packages either a release tag or the current commit.

In `release_tag` mode the tag must be a stable `vX.Y.Z` (the leading `v` is optional on manual input) that exists in the repository, and `[workspace.package].version` in the root `Cargo.toml` at that tag must equal it. The smoke and build jobs check out the tag's commit, so the Rust code, lockfile, and package templates match the tag exactly. Packaging helper scripts come from the workflow's own commit (`github.workflow_sha`), so a manual retry from an updated branch can apply packaging fixes to an existing tag without changing its source. A published release always uses this mode and uploads to that release.

In `current_ref` mode the run packages the commit it was started from and never publishes. The version comes from the workspace version, which may be a prerelease. Artifact names keep the full prerelease version, but the MSI uses only `major.minor.patch`, because WiX product versions cannot carry prerelease or build metadata.

A manual run defaults to `current_ref` with `dry_run` enabled. To publish manually, choose `release_tag`, give an existing stable tag, and set `dry_run` to false.

## Cutting a release

1. Set `[workspace.package].version` in the root `Cargo.toml` to the release version, build once so `Cargo.lock` records it (release builds use `--locked`), and merge both. The workflow rejects a tag that disagrees with the version.
2. Optionally rehearse: start `release-gui` manually on the branch with the defaults. The packages are kept as workflow artifacts for one day.
3. In GitHub Releases, choose **Draft a new release**, select or create the tag `vX.Y.Z` on the merged release commit, and write the release notes. You can save the draft and return to it later without starting a build.
4. Click **Publish release**. This starts the build; on success the same release holds the three packages, the three archives, and `checksums.sha256`. The workflow preserves your release notes and appends only the macOS note below.

Re-running the workflow for a tag replaces assets that have the same names.

## Packaging

Each platform job builds `localpaste-gui` in release mode and stages it as `localpaste` (`localpaste.exe` on Windows) next to the `LICENSE`. `release_gui_prepare.py` writes the effective packager config from `packaging/<platform>/packager.json` with the release version filled in, and `cargo-packager` produces the MSI, AppImage, or DMG and app bundle from it. `release_gui_collect.py` then names the assets, adds the ZIP or tarball of the staged binary (the app bundle on macOS), and the job uploads them.

Before upload the job checks its package: the MSI must contain `localpaste.exe` and its required Microsoft C++ runtime DLLs together after administrative extraction, the AppImage must report its runtime version and extract, and the DMG must pass `hdiutil verify`, plus a signature check of the app inside the DMG when the build is signed.

Windows binaries require the Microsoft C++ runtime used by ONNX Runtime. The prepare helper finds the x64 redistributable through `VCToolsRedistDir` or the installed Visual Studio C++ tools, stages its DLLs beside the executable, and maps them into the MSI's install directory. The ZIP contains the same DLLs. Both helpers reject a payload missing `msvcp140.dll`, `msvcp140_1.dll`, `vcruntime140.dll`, or `vcruntime140_1.dll`.

Windows packaging needs a WiX 3 toolchain. `release_gui_prepare.py` finds an installed one and asserts its major version, and the workflow installs WiX 3.14.1 from Chocolatey when no WiX 3 installation is found. That installation is only a preflight: `cargo-packager` ignores it and downloads its own WiX 3.11.2 into `WixTools` under `<cache-dir>/.cargo-packager`, as the pinned version's [WiX code](https://github.com/crabnebula-dev/cargo-packager/blob/cargo-packager-v0.11.8/crates/packager/src/package/wix/mod.rs) shows.

## macOS signing and notarization

Signing runs only when all of these repository secrets are set: `APPLE_SIGNING_CERT_BASE64`, `APPLE_SIGNING_CERT_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_APP_SPECIFIC_PASSWORD`, and `APPLE_TEAM_ID`. With them, the effective config sets `macos.signingIdentity` and the workflow passes credentials to cargo-packager using its `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_ID`, `APPLE_PASSWORD`, and `APPLE_TEAM_ID` environment variables. The packager signs, notarizes, and staples the app before putting it in the DMG, then signs the DMG. The workflow notarizes and staples the DMG and verifies the signature and staple of the app inside it. It never modifies the app after assembling the DMG. Without all six secrets, a `release_tag` run still publishes the macOS artifacts unsigned and unnotarized, and a `current_ref` run builds them unsigned.

When a release contains a DMG, the publish job appends a one-line Gatekeeper note to the release body, once. Its wording matches the instructions in [../release-gui.md](../release-gui.md#macos-gatekeeper); keep the two in step.

## Integrity controls

The workflows default to a read-only token, and only the publish job has `contents: write`. Every action is pinned to a full commit SHA with a trailing comment naming its release or branch, and `validate_workflow.py` rejects unpinned ones. The Rust toolchain and `cargo-packager` versions are pinned in both `release-gui.yml` and `verify-gui-packaging.yml` (the toolchain also in `rust-toolchain.toml`), so change them together.

## Where to change what

| To change | Edit |
| --- | --- |
| Runners, targets, job order, publishing, WiX installation | `.github/workflows/release-gui.yml` |
| Pinned toolchain and `cargo-packager` versions | `release-gui.yml`, `verify-gui-packaging.yml`, `rust-toolchain.toml` |
| Package formats, product name, identifier, minimum macOS version | `packaging/<platform>/packager.json` |
| Version injection, staging, WiX version check | `.github/scripts/release_gui_prepare.py` |
| Microsoft C++ redistributable discovery and required DLLs | `.github/scripts/release_windows_runtime.py` |
| App icons | `assets/icons/desktop_icon.png` (Linux, and the source of the macOS `.icns` generated in the workflow), `packaging/windows/localpaste.ico` (also embedded in the executable by `crates/localpaste_gui/build.rs`) |
| Asset names and archive contents | `.github/scripts/release_gui_collect.py` |
| Tag and version rules | `.github/scripts/release_versioning.py`, `normalize_release_tag.sh`, `normalize_packaging_tag.py`, `workspace_version.sh` |
| Invariants enforced on pull requests | `.github/scripts/validate_workflow.py` |
