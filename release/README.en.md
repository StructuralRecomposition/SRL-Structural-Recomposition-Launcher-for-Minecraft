# SRL release packages

[简体中文](README.md) · **English**

Current source version: **v0.2.2 local test build** (multiplayer center · dependency graph · Java auto-install)

GitHub Releases is authoritative; this directory only keeps local verification artifacts and notes.

| Platform | File |
|---|---|
| macOS (Intel + Apple Silicon) | `SRL_0.2.2_universal.dmg` |
| Windows 10/11 x64 | `SRL_0.2.2_windows-x64-portable.zip` |
| Linux x64 (Debian / Ubuntu) | `SRL_0.2.2_amd64.deb` |
| Linux x64 (universal) | `SRL_0.2.2_amd64.AppImage` |

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
sudo apt install ./SRL_0.2.2_amd64.deb
```

or run the AppImage. WebKitGTK 4.1 and GTK 3 are required.

## v0.2.2 local test build (unreleased)

- Multiplayer management center with host/guest modes, separated status, player events, room log, Server List Ping and a stoppable monitor.
- Mod dependency visualization with recursive Modrinth/CurseForge resolution, SVG graph, installed detection and safe installs.
- Java auto-completion installs Temurin when missing and continues launching without overwriting the global default.
- All four installers were built locally; no GitHub Release has been published yet.

## What's new in v0.2.1

- Bilingual UI (Simplified Chinese / English), Chinese by default, switchable in Settings.
- Installers bundle the official frpc; first use verifies SHA-256, extracts and runs it locally, so room creation no longer depends on GitHub.
- Modrinth `.mrpack` and CurseForge `manifest.json` modpack export.

## Data locations

| Platform | Path |
|---|---|
| macOS | `~/Library/Application Support/app.srlauncher.desktop` |
| Windows | `%APPDATA%\app.srlauncher.desktop` |
| Linux | `~/.local/share/app.srlauncher.desktop` |

Uninstalling the program does not delete launcher data.
