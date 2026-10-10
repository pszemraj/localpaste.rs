# GUI Releases

[GitHub Releases](https://github.com/pszemraj/localpaste.rs/releases) publishes the desktop GUI for Windows x86_64, Linux x86_64, and macOS on Apple Silicon (macOS 11 or later). Each platform comes as a package and as a plain archive, named `localpaste-<tag>-<platform>.<extension>`, where `<tag>` is the release version such as `vX.Y.Z`.

| Platform | Package | Archive |
| --- | --- | --- |
| Windows x86_64 | `localpaste-<tag>-windows-x86_64.msi` | `localpaste-<tag>-windows-x86_64.zip` |
| Linux x86_64 | `localpaste-<tag>-linux-x86_64.AppImage` | `localpaste-<tag>-linux-x86_64.tar.gz` |
| macOS Apple Silicon | `localpaste-<tag>-macos-aarch64.dmg` | `localpaste-<tag>-macos-aarch64.app.tar.gz` |

Every release also includes `checksums.sha256`, which lists a SHA-256 sum for each file above.

## Install A Release

**Windows.** Run the MSI installer. Alternatively, extract the ZIP and run `localpaste.exe`.

**Linux.** Make the AppImage executable and run it:

```bash
chmod +x localpaste-*.AppImage
./localpaste-*.AppImage
```

Alternatively, extract the `.tar.gz` and run `./localpaste`.

**macOS.** Open the DMG and drag `LocalPaste.app` to Applications. Alternatively, extract the `.app.tar.gz` and move the app bundle to Applications. If macOS refuses to open the app, see [macOS Gatekeeper](#macos-gatekeeper).

The `localpaste` executable inside the archives is the GUI under a shorter name. Releases do not include the standalone server (also called `localpaste` when built from source) or the `lpaste` CLI; build those from source.

## macOS Gatekeeper

macOS builds are signed and notarized only when the release was built with the maintainer's Apple signing credentials. Otherwise Gatekeeper blocks the first launch with a message that LocalPaste cannot be verified. To open it anyway, use either of these:

1. Try to open `LocalPaste.app` once, then open System Settings > Privacy & Security, find the message about LocalPaste, and choose Open Anyway.
2. Remove the quarantine flag from the installed app in a terminal, then open it normally: `xattr -cr /Applications/LocalPaste.app`

## Verify A Download

Download `checksums.sha256` next to the files and check them. On Linux, in the download directory:

```bash
sha256sum --check --ignore-missing checksums.sha256
```

On macOS, `shasum -a 256 <file>` prints the sum to compare against the matching line; on Windows, use `Get-FileHash <file> -Algorithm SHA256`.

## For Maintainers

How the artifacts are built, verified, and published is described in [dev/release-pipeline.md](dev/release-pipeline.md).
