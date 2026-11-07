SRL 0.2.0 - Windows x64 portable build

This is an unsigned cross-compiled test build. Run Install.cmd (right-click ->
Run, administrator rights are not required) to copy SRL to %LOCALAPPDATA%\SRL
and create Desktop/Start Menu shortcuts. Or run sr-launcher.exe directly from
this folder.

Requirements:
- Windows 10/11 x64
- Microsoft Edge WebView2 Runtime (normally included with Edge)
- No admin rights required

Minecraft Multiplayer in this release:
- New Minecraft Multiplayer page with 52FRP API token connection.
- Java Edition TCP and Bedrock Edition UDP tunnel creation.
- Automatic local LAN port detection and manual override.
- Node selection, FRPC lifecycle, public address copy, stop/delete separation.
- Create an API token at https://www.52frp.com/user/#/user-center
  (Personal Center -> API Access Token, starts with hz_).
- SRL stores the token in Windows Credential Manager, never in settings or logs.

Troubleshooting "localhost refused to connect" / blank window:
1. Extract the whole ZIP to a normal folder first; do not run the EXE from
   inside the ZIP preview.
2. Install or repair the WebView2 Evergreen Runtime:
   https://developer.microsoft.com/microsoft-edge/webview2/
3. If this cloud/office PC uses a system proxy, launch from cmd with:
   set WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--proxy-bypass-list=tauri.localhost;localhost;127.0.0.1
   sr-launcher.exe
4. This package is a production build (embedded UI). If you still see
   localhost:1420, you are running a development build by mistake.

This build is unsigned, so Windows SmartScreen may warn. Verify the SHA-256
next to the ZIP before running.

The package contains no settings, accounts, chat history, Java, Minecraft
instances, logs or API keys. First launch creates a clean data directory under
%APPDATA%\app.srlauncher.desktop.

Uninstall.cmd removes the program; your launcher data is intentionally kept.
