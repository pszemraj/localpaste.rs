# GUI Releases

[GitHub Releases](https://github.com/pszemraj/localpaste.rs/releases) publishes the desktop GUI for Windows x86_64, Linux x86_64, and macOS on Apple Silicon (macOS 11 or later). Each platform comes as a package and as a plain archive, named `localpaste-<tag>-<platform>.<extension>`, where `<tag>` is the release version such as `vX.Y.Z`.

| Platform | Package | Archive |
| --- | --- | --- |
| Windows x86_64 | `localpaste-<tag>-windows-x86_64.msi` | `localpaste-<tag>-windows-x86_64.zip` |
| Linux x86_64 | `localpaste-<tag>-linux-x86_64.AppImage` | `localpaste-<tag>-linux-x86_64.tar.gz` |
| macOS Apple Silicon | `localpaste-<tag>-macos-aarch64.dmg` | `localpaste-<tag>-macos-aarch64.app.tar.gz` |

Every release also includes `checksums.sha256`, which lists a SHA-256 sum for each file above.

## Install A Release

**Windows.** Run the MSI installer. Alternatively, extract the entire ZIP and run `localpaste.exe`; keep its bundled runtime DLLs beside the executable.

**Linux.** Make the AppImage executable and run it:

```bash
chmod +x localpaste-*.AppImage
./localpaste-*.AppImage
```

Alternatively, extract the `.tar.gz` and run `./localpaste`.

**macOS.** Open the DMG and drag `LocalPaste.app` to Applications. Alternatively, extract the `.app.tar.gz` and move the app bundle to Applications. If macOS refuses to open the app, see [macOS Gatekeeper](#macos-gatekeeper).

The `localpaste` executable inside the archives is the GUI under a shorter name. Releases do not include the standalone server (also called `localpaste` when built from source) or the `lpaste` CLI; build those from source.

## macOS Gatekeeper

> [!TIP]
> macOS packages are intentionally unsigned and unnotarized by default. Open the DMG and drag **LocalPaste.app** to **Applications**.
>
> If macOS blocks the installed app, run this in Terminal, then open the app again:
>
> ```sh
> xattr -dr com.apple.quarantine "/Applications/LocalPaste.app"
> ```
>
> If the downloaded DMG itself is blocked, remove its quarantine flag first, then reopen it (replace `<tag>` with the version you downloaded):
>
> ```sh
> xattr -d com.apple.quarantine "$HOME/Downloads/localpaste-<tag>-macos-aarch64.dmg"
> ```
>
> Adjust the paths if you downloaded or installed elsewhere. Alternatively, try opening the app once, then choose **Open Anyway** in **System Settings > Privacy & Security** ([Apple's instructions](https://support.apple.com/en-us/102445)).

## Verify A Download

Download `checksums.sha256` next to the files and check them. On Linux, in the download directory:

```bash
sha256sum --check --ignore-missing checksums.sha256
```

On macOS, `shasum -a 256 <file>` prints the sum to compare against the matching line; on Windows, use `Get-FileHash <file> -Algorithm SHA256`.

## For Maintainers

How the artifacts are built, verified, and published is described in [dev/release-pipeline.md](dev/release-pipeline.md).
