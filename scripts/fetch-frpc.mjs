#!/usr/bin/env node
// Fetch the official frp release archives that ship inside SRL installers.
//
// The lock file (src-tauri/resources/frpc/frpc.lock.json) pins the exact
// release and SHA-256 for every supported target. Builds are reproducible and
// never need the GitHub API once the lock exists. Run with --update to move to
// the latest upstream release.
import { createHash } from "node:crypto";
import { createWriteStream } from "node:fs";
import { mkdir, readFile, rename, unlink, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const RESOURCE_ROOT = join(ROOT, "src-tauri", "resources", "frpc");
const LOCK_PATH = join(RESOURCE_ROOT, "frpc.lock.json");
const API = "https://api.github.com/repos/fatedier/frp/releases";
const SUPPORTED = new Set([
  "darwin-amd64",
  "darwin-arm64",
  "windows-amd64",
  "windows-arm64",
  "linux-amd64",
  "linux-arm64",
]);
const MIRRORS = [
  "",
  "https://ghproxy.net/",
  "https://gh-proxy.com/",
];

const log = (...args) => console.log("[frpc]", ...args);

function parseArgs(argv) {
  const out = { update: false, targets: null, version: null };
  for (const arg of argv) {
    if (arg === "--update") out.update = true;
    else if (arg.startsWith("--targets=")) {
      out.targets = arg.slice("--targets=".length).split(",").filter(Boolean);
    } else if (arg.startsWith("--version=")) {
      out.version = arg.slice("--version=".length).replace(/^v/, "");
      out.update = true;
    }
  }
  return out;
}

function hostTargets() {
  const platform =
    process.platform === "darwin" ? "darwin" : process.platform === "win32" ? "windows" : "linux";
  const arch = process.arch === "arm64" ? "arm64" : "amd64";
  return [`${platform}-${arch}`];
}

function tripleTargets(triple) {
  if (!triple) return null;
  if (triple.includes("universal-apple-darwin")) return ["darwin-amd64", "darwin-arm64"];
  const platform = triple.includes("windows")
    ? "windows"
    : triple.includes("apple-darwin")
      ? "darwin"
      : triple.includes("linux")
        ? "linux"
        : null;
  if (!platform) return null;
  const arch = /aarch64|arm64/.test(triple) ? "arm64" : "amd64";
  return [`${platform}-${arch}`];
}

function requestedTargets(args) {
  const fromArgs = args.targets?.map((value) => value.trim()).filter(Boolean);
  const fromEnv = tripleTargets(process.env.TAURI_ENV_TARGET_TRIPLE);
  const targets = fromArgs?.length ? fromArgs : fromEnv ?? hostTargets();
  const unknown = targets.filter((target) => !SUPPORTED.has(target));
  if (unknown.length) {
    throw new Error(`unsupported frpc target(s): ${unknown.join(", ")}`);
  }
  return [...new Set(targets)];
}

async function fetchJson(url) {
  const response = await fetch(url, {
    headers: {
      Accept: "application/vnd.github+json",
      "User-Agent": "srl-build",
    },
    signal: AbortSignal.timeout(20_000),
  });
  if (!response.ok) throw new Error(`${url} -> HTTP ${response.status}`);
  return response.json();
}

async function fetchText(url) {
  const response = await fetch(url, {
    headers: { "User-Agent": "srl-build" },
    signal: AbortSignal.timeout(20_000),
  });
  if (!response.ok) throw new Error(`${url} -> HTTP ${response.status}`);
  return response.text();
}

async function updateLock(pinnedVersion) {
  const version = pinnedVersion;
  log(`resolving frp ${version ? `v${version}` : "latest"} release`);
  const release = await fetchJson(version ? `${API}/tags/v${version}` : `${API}/latest`);
  const resolved = String(release.tag_name ?? "").replace(/^v/, "");
  if (!/^\d+\.\d+\.\d+$/.test(resolved)) {
    throw new Error(`unexpected frp tag: ${release.tag_name}`);
  }
  const checksumsUrl =
    release.assets?.find((asset) => asset.name === "frp_sha256_checksums.txt")
      ?.browser_download_url ??
    `https://github.com/fatedier/frp/releases/download/v${resolved}/frp_sha256_checksums.txt`;
  const checksumText = await fetchText(checksumsUrl);
  const checksums = new Map();
  for (const line of checksumText.split(/\r?\n/)) {
    const match = line.trim().match(/^([0-9a-fA-F]{64})\s+\*?(.+)$/);
    if (match) checksums.set(match[2], match[1].toLowerCase());
  }
  const assets = {};
  for (const asset of release.assets ?? []) {
    const match = asset.name.match(/^frp_\d+\.\d+\.\d+_(.+?)_(amd64|arm64)\.(tar\.gz|zip)$/);
    if (!match) continue;
    const key = `${match[1]}-${match[2]}`;
    if (!SUPPORTED.has(key)) continue;
    const sha256 = checksums.get(asset.name);
    if (!sha256) throw new Error(`missing checksum for ${asset.name}`);
    assets[key] = { name: asset.name, sha256 };
  }
  const missing = [...SUPPORTED].filter((key) => !assets[key]);
  if (missing.length) throw new Error(`release is missing assets: ${missing.join(", ")}`);
  const lock = {
    version: resolved,
    generatedAt: new Date().toISOString(),
    assets,
  };
  await mkdir(RESOURCE_ROOT, { recursive: true });
  await writeFile(LOCK_PATH, `${JSON.stringify(lock, null, 2)}\n`, "utf8");
  log(`locked frp ${resolved}`);
  return lock;
}

async function readLock() {
  const text = await readFile(LOCK_PATH, "utf8");
  return JSON.parse(text);
}

async function fileSha256(path) {
  try {
    const data = await readFile(path);
    return createHash("sha256").update(data).digest("hex");
  } catch {
    return null;
  }
}

async function download(url, target) {
  const tmp = `${target}.part`;
  const response = await fetch(url, {
    headers: { "User-Agent": "srl-build" },
    signal: AbortSignal.timeout(180_000),
  });
  if (!response.ok || !response.body) {
    throw new Error(`${url} -> HTTP ${response.status}`);
  }
  await pipeline(Readable.fromWeb(response.body), createWriteStream(tmp));
  await rename(tmp, target);
}

async function ensureAsset(lock, key) {
  const asset = lock.assets[key];
  if (!asset) {
    throw new Error(
      `frpc.lock.json has no ${key} asset; run "node scripts/fetch-frpc.mjs --update"`,
    );
  }
  const [platform, arch] = key.split("-");
  const dir = join(RESOURCE_ROOT, platform, arch);
  const target = join(dir, asset.name);
  await mkdir(dir, { recursive: true });
  const existing = await fileSha256(target);
  if (existing === asset.sha256) {
    log(`${key}: ${asset.name} already present`);
    return;
  }
  if (existing) {
    log(`${key}: checksum mismatch, re-downloading`);
    await unlink(target).catch(() => {});
  }
  const url = `https://github.com/fatedier/frp/releases/download/v${lock.version}/${asset.name}`;
  let lastError = null;
  for (const prefix of MIRRORS) {
    try {
      log(`${key}: downloading ${prefix || "github.com"}${asset.name}`);
      await download(`${prefix}${url}`, target);
      const actual = await fileSha256(target);
      if (actual !== asset.sha256) {
        throw new Error(`checksum mismatch (expected ${asset.sha256}, got ${actual})`);
      }
      log(`${key}: verified ${asset.sha256.slice(0, 12)}…`);
      return;
    } catch (error) {
      lastError = error;
      await unlink(target).catch(() => {});
      log(`${key}: ${error.message}`);
    }
  }
  throw lastError ?? new Error(`failed to download ${asset.name}`);
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  let lock;
  try {
    lock = args.update ? await updateLock(args.version) : await readLock();
  } catch (error) {
    if (args.update) throw error;
    log(`no usable lock (${error.message}); resolving latest`);
    lock = await updateLock(args.version);
  }
  const targets = requestedTargets(args);
  log(`frp ${lock.version}; targets: ${targets.join(", ")}`);
  for (const target of targets) {
    await ensureAsset(lock, target);
  }
}

main().catch((error) => {
  console.error(`[frpc] error: ${error.message}`);
  process.exit(1);
});
