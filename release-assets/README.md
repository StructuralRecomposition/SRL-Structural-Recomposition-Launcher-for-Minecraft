# Release assets

本目录存放 `v0.1.0` 的发布产物，供 `.github/workflows/release.yml` 在推送 tag 时上传到 GitHub Releases。

这些是构建产物，不是源代码：

- `SRL_0.1.0_universal.dmg`
- `SRL_0.1.0_windows-x64-portable.zip`
- `SRL_0.1.0_amd64.deb`
- `SRL-0.1.0-source.tar.gz`
- `SHA256SUMS`

发布完成后，本目录可以从默认分支移除；GitHub Release 中的附件不会受影响。
