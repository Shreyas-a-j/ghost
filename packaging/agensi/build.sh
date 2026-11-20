#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

command -v zip >/dev/null 2>&1 || {
  echo "error: zip is required" >&2
  exit 1
}

rm -f ghost.zip
(
  cd ghost
  zip -qr ../ghost.zip .
)

echo "built $(pwd)/ghost.zip"
unzip -l ghost.zip
