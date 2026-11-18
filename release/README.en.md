# SRL release packages

[简体中文](README.md) · **English**

Current source version: **v0.2.6 interface system refresh** (new design system · simpler settings · launcher update check)

GitHub Releases is authoritative; this directory only keeps local verification artifacts and notes.

| Platform | File |
|---|---|
| macOS (Intel + Apple Silicon) | `SRL_0.2.6_universal.dmg` |
| Windows 10/11 x64 | `SRL_0.2.6_windows-x64-portable.zip` |
| Linux x64 (Debian / Ubuntu) | `SRL_0.2.6_amd64.deb` |
| Linux x64 (universal) | `SRL_0.2.6_amd64.AppImage` |

Checksums are in `SHA256SUMS`.

## First run

### macOS

Open the DMG and drag SRL into Applications. The build is unsigned and not notarized, so Gatekeeper may warn. If needed:

```bash
xattr -dr com.apple.quarantine /Applications/SRL.app
```

### Windows

Extract the whole ZIP and run `Install.cmd` (no administrator rights needed), or run `sr-launcher.exe` directly. WebView2 Runtime is required.

### Linux

```bash
sudo apt install ./SRL_0.2.6_amd64.deb
```

or run the AppImage. WebKitGTK 4.1 and GTK 3 are required.

## Note

- Installers are built by GitHub Actions on the matching tag. Local files are for verification only and do not replace release assets.
- This release is unsigned and not notarized; verify downloads against `SHA256SUMS` in the release assets.

## What's new in v0.2.6

- New interface: consistent type, spacing, colour and components; primary buttons are now monochrome.
- Simpler settings: collapsible download/network/API/storage groups; 52FRP, CurseForge and DeepSeek share one “API & integrations” section.
- Fixed overflowing path copy buttons and clipped account avatars; instance import now uses a native file picker.
- Launcher update check: checks GitHub Releases on startup, with mirror download, SHA-256 verification, install and restart.
- macOS universal DMG, Windows x64 portable ZIP, Linux deb / AppImage.

## Data locations

| Platform | Path |
|---|---|
| macOS | `~/Library/Application Support/app.srlauncher.desktop` |
| Windows | `%APPDATA%\app.srlauncher.desktop` |
| Linux | `~/.local/share/app.srlauncher.desktop` |

Uninstalling the program does not delete launcher data.