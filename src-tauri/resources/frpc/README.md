# Bundled FRPC

`frpc.lock.json` pins the official fatedier/frp release and the SHA-256 of every
supported platform archive. The archives themselves are **not** committed.

Builds fetch the archive for the current target automatically via
`scripts/fetch-frpc.mjs` (wired into `beforeBuildCommand`). Manual usage:

```bash
node scripts/fetch-frpc.mjs                       # current host target
node scripts/fetch-frpc.mjs --targets=darwin-amd64,darwin-arm64
node scripts/fetch-frpc.mjs --version=0.71.0      # re-pin a release
node scripts/fetch-frpc.mjs --update              # move to latest upstream
```

The installer ships the compressed archive; at first use SRL verifies its
SHA-256, extracts `frpc` into the app data directory and runs it from there.
Shipping an archive instead of a bare executable keeps macOS code signing and
Windows portable packages simple.
