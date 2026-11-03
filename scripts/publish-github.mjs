#!/usr/bin/env node
/**
 * Publish SRL v0.1.0 to GitHub using only the REST API.
 *
 * Usage:
 *   GH_TOKEN=... node scripts/publish-github.mjs [repo-name]
 *   node scripts/publish-github.mjs --dry-run
 *
 * The token can also be stored in <workspace>/.gh_token (one line).
 * Requires a token with `repo` scope (classic) or Contents/Releases write
 * permission (fine-grained) for the target repository.
 */

import { execFileSync } from "node:child_process";
import { readFileSync, existsSync, readlinkSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const API = "https://api.github.com";
const UPLOAD = "https://uploads.github.com";
const TAG = "v0.1.0";
const RELEASE_NAME = "SRL v0.1.0 — 首个公开发布";
const ASSET_DIR = join(ROOT, "dist/release-v0.1.0");
const NOTES = join(ROOT, "release/RELEASE_NOTES_v0.1.0.md");
const DRY_RUN = process.argv.includes("--dry-run");
const repoName = process.argv.slice(2).find((a) => !a.startsWith("-")) || "SRL";

function token() {
  if (process.env.GH_TOKEN) return process.env.GH_TOKEN.trim();
  for (const p of [join(ROOT, ".gh_token"), join(process.env.HOME || "", ".gh_token")]) {
    if (existsSync(p)) return readFileSync(p, "utf8").trim();
  }
  throw new Error("Missing GH_TOKEN (or .gh_token with one line)");
}

async function api(path, { method = "GET", body, headers = {} } = {}, tok) {
  const res = await fetch(API + path, {
    method,
    headers: {
      Accept: "application/vnd.github+json",
      "X-GitHub-Api-Version": "2022-11-28",
      Authorization: `Bearer ${tok}`,
      "User-Agent": "srl-release-script",
      ...headers,
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await res.text();
  if (!res.ok) throw new Error(`${method} ${path} -> ${res.status}\n${text}`);
  return text ? JSON.parse(text) : null;
}

function listFiles() {
  const raw = execFileSync("git", ["ls-files", "-s", "-z"], { cwd: ROOT });
  const entries = [];
  for (const record of raw.toString("utf8").split("\0")) {
    if (!record) continue;
    const tab = record.indexOf("\t");
    if (tab < 0) continue;
    const [mode, sha, stage] = record.slice(0, tab).split(" ");
    const path = record.slice(tab + 1);
    if (path.startsWith("release/")) {
      // Keep release notes/readme in the repository, but never the binaries.
      if (path !== "release/README.md" && path !== "release/RELEASE_NOTES_v0.1.0.md") continue;
    }
    if (path.startsWith("dist/") || path.startsWith("node_modules/") || path.startsWith("target/")) continue;
    if (path === "tsconfig.tsbuildinfo") continue;
    entries.push({ mode, sha, stage, path });
  }
  return entries;
}

async function main() {
  const tok = DRY_RUN ? "dry-run" : token();
  const files = listFiles();
  const total = files.reduce((n, f) => n + readFileSync(join(ROOT, f.path)).length, 0);
  console.log(`Repository: ${repoName}`);
  console.log(`Files: ${files.length} (${(total / 1024 / 1024).toFixed(2)} MiB)`);
  console.log(`Tag: ${TAG}`);
  console.log(`Assets: ${ASSET_DIR}`);
  if (DRY_RUN) {
    for (const f of files) console.log(`  ${f.mode} ${f.path}`);
    return;
  }

  const user = await api("/user", {}, tok);
  const owner = user.login;
  console.log(`Authenticated as ${owner}`);

  let repo;
  try {
    repo = await api(`/repos/${owner}/${repoName}`, {}, tok);
    console.log(`Repository exists: ${repo.html_url}`);
  } catch (err) {
    if (!String(err.message).includes("404")) throw err;
    repo = await api(
      "/user/repos",
      {
        method: "POST",
        body: {
          name: repoName,
          description:
            "SRL — an independent, local-first Minecraft: Java Edition launcher built with Rust, Tauri 2, React and TypeScript.",
          private: false,
          has_issues: true,
          has_projects: false,
          has_wiki: false,
          auto_init: false,
        },
      },
      tok,
    );
    console.log(`Created repository: ${repo.html_url}`);
  }

  let commitSha;
  try {
    const ref = await api(`/repos/${owner}/${repoName}/git/ref/heads/main`, {}, tok);
    commitSha = ref.object.sha;
    console.log(`Using existing main: ${commitSha}`);
  } catch (err) {
    if (!String(err.message).includes("404")) throw err;
    console.log("Uploading blobs...");
    const tree = [];
    let done = 0;
    for (const file of files) {
      const content =
        file.mode === "120000"
          ? readlinkSync(join(ROOT, file.path))
          : await readFile(join(ROOT, file.path));
      const blob = await api(
        `/repos/${owner}/${repoName}/git/blobs`,
        { method: "POST", body: { content: content.toString("base64"), encoding: "base64" } },
        tok,
      );
      tree.push({ path: file.path, mode: file.mode, type: "blob", sha: blob.sha });
      done += 1;
      if (done % 25 === 0 || done === files.length) console.log(`  ${done}/${files.length}`);
    }
    console.log("Creating tree and commit...");
    const treeObj = await api(`/repos/${owner}/${repoName}/git/trees`, { method: "POST", body: { tree } }, tok);
    const commit = await api(
      `/repos/${owner}/${repoName}/git/commits`,
      {
        method: "POST",
        body: { message: "SRL v0.1.0 — first public release", tree: treeObj.sha, parents: [] },
      },
      tok,
    );
    commitSha = commit.sha;
    await api(
      `/repos/${owner}/${repoName}/git/refs`,
      { method: "POST", body: { ref: "refs/heads/main", sha: commitSha } },
      tok,
    );
    console.log(`Created main: ${commitSha}`);
  }

  try {
    await api(`/repos/${owner}/${repoName}/git/ref/tags/${TAG}`, {}, tok);
    console.log(`Tag ${TAG} already exists`);
  } catch (err) {
    if (!String(err.message).includes("404")) throw err;
    const tag = await api(
      `/repos/${owner}/${repoName}/git/tags`,
      {
        method: "POST",
        body: { tag: TAG, message: "SRL v0.1.0 — first public release", object: commitSha, type: "commit" },
      },
      tok,
    );
    await api(
      `/repos/${owner}/${repoName}/git/refs`,
      { method: "POST", body: { ref: `refs/tags/${TAG}`, sha: tag.sha } },
      tok,
    );
    console.log(`Created tag ${TAG}`);
  }

  let release;
  try {
    release = await api(`/repos/${owner}/${repoName}/releases/tags/${TAG}`, {}, tok);
    console.log(`Release exists: ${release.html_url}`);
  } catch (err) {
    if (!String(err.message).includes("404")) throw err;
    release = await api(
      `/repos/${owner}/${repoName}/releases`,
      {
        method: "POST",
        body: {
          tag_name: TAG,
          name: RELEASE_NAME,
          body: readFileSync(NOTES, "utf8"),
          draft: false,
          prerelease: false,
        },
      },
      tok,
    );
    console.log(`Created release: ${release.html_url}`);
  }

  const assets = [
    "SRL_0.1.0_universal.dmg",
    "SRL_0.1.0_windows-x64-portable.zip",
    "SRL_0.1.0_amd64.deb",
    "SRL-0.1.0-source.tar.gz",
    "SHA256SUMS",
  ];
  const existing = new Set((release.assets || []).map((a) => a.name));
  for (const name of assets) {
    if (existing.has(name)) {
      console.log(`Asset exists: ${name}`);
      continue;
    }
    const path = join(ASSET_DIR, name);
    const bytes = await readFile(path);
    console.log(`Uploading ${name} (${(bytes.length / 1024 / 1024).toFixed(1)} MiB)...`);
    const res = await fetch(
      `${UPLOAD}/repos/${owner}/${repoName}/releases/${release.id}/assets?name=${encodeURIComponent(name)}`,
      {
        method: "POST",
        headers: {
          Accept: "application/vnd.github+json",
          Authorization: `Bearer ${tok}`,
          "Content-Type": "application/octet-stream",
          "User-Agent": "srl-release-script",
        },
        body: bytes,
      },
    );
    if (!res.ok) throw new Error(`Upload ${name} -> ${res.status}\n${await res.text()}`);
    console.log(`  ok: ${name}`);
  }

  console.log(`\nDone: ${repo.html_url}/releases/tag/${TAG}`);
}

main().catch((err) => {
  console.error(err.message || err);
  process.exit(1);
});
