#!/usr/bin/env bash
# Copia el sitio estático website/ → docs/ (raíz publicada en GitHub Pages legacy).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC="$REPO_ROOT/website"
DEST="$REPO_ROOT/docs"

for f in index.html fisica.html paridad-or.html .nojekyll; do
  cp -a "$SRC/$f" "$DEST/$f"
done
mkdir -p "$DEST/css"
cp -a "$SRC/css/style.css" "$DEST/css/style.css"

echo "OK: website/ → docs/ ($(wc -l < "$DEST/fisica.html") líneas fisica.html)"
