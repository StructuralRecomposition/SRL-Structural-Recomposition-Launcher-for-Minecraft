#!/usr/bin/env node
/**
 * Update the GitHub repository About/description, homepage, and topics.
 *
 * Usage:
 *   GH_TOKEN=... node scripts/update-repo-metadata.mjs
 *   node scripts/update-repo-metadata.mjs --dry-run
 *
 * Token scopes:
 *   - Classic PAT: `repo`
 *   - Fine-grained PAT: Administration (read/write) + Metadata (read)
 */

import { readFileSync, existsSync } from "node:fs";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const API = "https://api.github.com";
const REPO = "sry8n5rjz5-debug/SRL";
const OWNER = REPO.split("/")[0];

const DESCRIPTION =
  "SRL (Structural Recomposition Launcher) — an independent, local-first Minecraft: Java Edition launcher built with Rust, Tauri 2, React and TypeScript. 本地优先、下载可验证、启动真实、错误可诊断。";
const HOMEPAGE = "https://github.com/sry8n5rjz5-debug/SRL/releases/latest";

const TOPICS = [
  "minecraft",
  "minecraft-launcher",
  "launcher",
  "tauri",
  "rust",
  "react",
  "typescript",
  "modrinth",
  "curseforge",
  "fabric",
  "forge",
  "neoforge",
  "quilt",
  "java",
  "cross-platform",
  "desktop-app",
  "game-launcher",
  "local-first",
  "open-source",
  "macos",
];

const DRY_RUN = process.argv.includes("--dry-run");

function token() {
  if (process.env.GH_TOKEN) return process.env.GH_TOKEN.trim();
  for (const p of [join(ROOT, ".gh_token"), join(process.env.HOME || "", ".gh_token")]) {
    if (existsSync(p)) return readFileSync(p, "utf8").trim();
  }
  throw new Error("Missing GH_TOKEN (or .gh_token with one line)");
}

async function api(path, { method = "GET", body } = {}, tok) {
  const res = await fetch(API + path, {
    method,
    headers: {
      Accept: "application/vnd.github+json",
      "X-GitHub-Api-Version": "2022-11-28",
      Authorization: `Bearer ${tok}`,
      "User-Agent": "srl-repo-metadata",
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await res.text();
  if (!res.ok) throw new Error(`${method} ${path} -> ${res.status}\n${text}`);
  return text ? JSON.parse(text) : null;
}

async function main() {
  console.log(`Repository: ${REPO}`);
  console.log(`Description: ${DESCRIPTION}`);
  console.log(`Homepage: ${HOMEPAGE}`);
  console.log(`Topics (${TOPICS.length}): ${TOPICS.join(", ")}`);
  if (DRY_RUN) return;

  const tok = token();
  const user = await api("/user", {}, tok);
  if (user.login !== OWNER) {
    throw new Error(`Token belongs to ${user.login}, expected ${OWNER}`);
  }

  await api(
    `/repos/${REPO}`,
    { method: "PATCH", body: { description: DESCRIPTION, homepage: HOMEPAGE } },
    tok,
  );
  await api(`/repos/${REPO}/topics`, { method: "PUT", body: { names: TOPICS } }, tok);

  const repo = await api(`/repos/${REPO}`, {}, tok);
  const topics = await api(`/repos/${REPO}/topics`, {}, tok);
  console.log("\nUpdated:");
  console.log(`  description: ${repo.description}`);
  console.log(`  homepage:    ${repo.homepage}`);
  console.log(`  topics:      ${(topics.names || []).join(", ")}`);
}

main().catch((err) => {
  console.error(err.message || err);
  process.exit(1);
});
