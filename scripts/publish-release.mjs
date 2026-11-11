#!/usr/bin/env node
/**
 * Publish a versioned GitHub Release for SRL.
 *
 * Usage:
 *   GH_TOKEN=... node scripts/publish-release.mjs v0.2.2
 *
 * The token can also be stored in <repo>/.gh_token (one line, never commit it).
 * Required fine-grained permissions for the target repository:
 *   Contents: Read and write
 */

import { existsSync, readFileSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { join, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const REPO =
  process.env.GH_REPO ||
  "StructuralRecomposition/SRL-Structural-Recomposition-Launcher-for-Minecraft";
const API = "https://api.github.com";
const UPLOAD = "https://uploads.github.com";
const tag = process.argv[2] || "v0.2.2";
const version = tag.replace(/^v/, "");
const notesPath = join(ROOT, "release", `RELEASE_NOTES_${tag}.md`);
const assetDir = join(ROOT, "release", tag);

function token() {
  if (process.env.GH_TOKEN) return process.env.GH_TOKEN.trim();
  for (const path of [join(ROOT, ".gh_token"), join(process.env.HOME || "", ".gh_token")]) {
    if (existsSync(path)) return readFileSync(path, "utf8").trim();
  }
  throw new Error("Missing GH_TOKEN (or .gh_token with one line)");
}

async function api(path, { method = "GET", body, headers = {} } = {}) {
  const response = await fetch(API + path, {
    method,
    headers: {
      Accept: "application/vnd.github+json",
      "X-GitHub-Api-Version": "2022-11-28",
      Authorization: `Bearer ${token()}`,
      "User-Agent": "srl-release-script",
      ...headers,
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await response.text();
  if (!response.ok) throw new Error(`${method} ${path} -> ${response.status}\n${text}`);
  return text ? JSON.parse(text) : null;
}

const names = [
  `SRL_${version}_universal.dmg`,
  `SRL_${version}_windows-x64-portable.zip`,
  `SRL_${version}_amd64.deb`,
  `SRL_${version}_amd64.AppImage`,
  `SRL-${version}-source.tar.gz`,
  "SHA256SUMS",
];

async function main() {
  if (!existsSync(notesPath)) throw new Error(`Missing release notes: ${notesPath}`);
  const body = readFileSync(notesPath, "utf8");
  const assets = names
    .map((name) => join(assetDir, name))
    .filter((path) => existsSync(path));
  if (assets.length === 0) throw new Error(`No assets found in ${assetDir}`);

  const releaseName = `SRL ${tag} — Structural Recomposition Launcher for Minecraft`;
  let release;
  try {
    release = await api(`/repos/${REPO}/releases/tags/${tag}`);
    console.log(`Release exists: ${release.html_url}`);
    release = await api(`/repos/${REPO}/releases/${release.id}`, {
      method: "PATCH",
      body: { name: releaseName, body, draft: false, prerelease: false, make_latest: "true" },
    });
    console.log("Updated title and bilingual notes");
  } catch (error) {
    if (!String(error.message).includes("404")) throw error;
    release = await api(`/repos/${REPO}/releases`, {
      method: "POST",
      body: {
        tag_name: tag,
        target_commitish: "main",
        name: releaseName,
        body,
        draft: false,
        prerelease: false,
        make_latest: "true",
      },
    });
    console.log(`Created release: ${release.html_url}`);
  }

  const existing = new Set((release.assets || []).map((asset) => asset.name));
  for (const path of assets) {
    const name = path.split("/").pop();
    if (existing.has(name)) {
      console.log(`Asset exists: ${name}`);
      continue;
    }
    const bytes = await readFile(path);
    console.log(`Uploading ${name} (${(bytes.length / 1024 / 1024).toFixed(1)} MiB)...`);
    const response = await fetch(
      `${UPLOAD}/repos/${REPO}/releases/${release.id}/assets?name=${encodeURIComponent(name)}`,
      {
        method: "POST",
        headers: {
          Accept: "application/vnd.github+json",
          Authorization: `Bearer ${token()}`,
          "Content-Type": "application/octet-stream",
          "User-Agent": "srl-release-script",
        },
        body: bytes,
      },
    );
    if (!response.ok) throw new Error(`Upload ${name} -> ${response.status}\n${await response.text()}`);
    console.log(`  ok: ${name}`);
  }

  console.log(`\nDone: https://github.com/${REPO}/releases/tag/${tag}`);
}

main().catch((error) => {
  console.error(error.message || error);
  process.exit(1);
});
