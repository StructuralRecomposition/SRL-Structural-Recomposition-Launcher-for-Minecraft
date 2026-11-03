#!/usr/bin/env bash
# Fail if a packaged app contains user data, test fixtures or AI keys.
set -euo pipefail
TARGET="${1:?usage: verify-clean-bundle.sh <app-or-bundle-dir>}"
FAIL=0

echo "Checking bundle: $TARGET"

if [ ! -e "$TARGET" ]; then
  echo "Bundle path not found: $TARGET" >&2
  exit 1
fi

while IFS= read -r -d '' path; do
  echo "  forbidden user file: $path"
  FAIL=1
done < <(find "$TARGET" -type f \( \
  -name 'settings.json' -o -name 'accounts.json' -o -name 'instance.json' \
  -o -name 'assistant-audit.jsonl' -o -name 'skins.json' -o -name 'tasks.json' \
  -o -name '*.log' -o -name '*.log.gz' -o -name '*.jar' -o -name '*.zip' \) -print0)

for dir in minecraft instances java skins cache logs assistant; do
  while IFS= read -r -d '' path; do
    echo "  forbidden user dir: $path"
    FAIL=1
  done < <(find "$TARGET" -type d -name "$dir" -print0)
done

if grep -rIlE 'sk-[A-Za-z0-9]{20,}' "$TARGET" >/dev/null 2>&1; then
  echo "  forbidden: DeepSeek-style API key pattern found in bundle"
  FAIL=1
fi

if grep -rIl 'fixtureBootstrap\|SkinUser\|Oneblock Test' "$TARGET" >/dev/null 2>&1; then
  echo "  forbidden: test fixture data found in bundle"
  FAIL=1
fi

if [ "$FAIL" -ne 0 ]; then
  echo "Bundle is NOT clean." >&2
  exit 1
fi
echo "Bundle is clean."
