import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

const roots = ["crates", "src", "src-tauri", "scripts"];
const skip = new Set(["target", "node_modules", "dist", ".git"]);
const keyPattern = /sk-[A-Za-z0-9_-]{20,}/;
let failed = false;

function walk(dir) {
  for (const name of readdirSync(dir)) {
    if (skip.has(name)) continue;
    const path = join(dir, name);
    const stat = statSync(path);
    if (stat.isDirectory()) {
      walk(path);
    } else if (
      stat.isFile() &&
      /\.(rs|ts|tsx|js|mjs|json|toml|md|yml|yaml|sh)$/.test(name)
    ) {
      const text = readFileSync(path, "utf8");
      if (keyPattern.test(text)) {
        console.error(`DeepSeek-style API key found: ${path}`);
        failed = true;
      }
    }
  }
}

for (const root of roots) walk(root);
if (failed) process.exit(1);
console.log("No DeepSeek API key found in source.");
