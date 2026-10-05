#!/usr/bin/env bash
# Copia el sitio estático website/ → docs/ (raíz publicada en GitHub Pages legacy).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC="$REPO_ROOT/website"
DEST="$REPO_ROOT/docs"

python3 "$REPO_ROOT/scripts/build_website.py"
for f in "$SRC"/*.html "$SRC/.nojekyll"; do
  cp -a "$f" "$DEST/$(basename "$f")"
done
for directory in css js assets; do
  mkdir -p "$DEST/$directory"
  cp -a "$SRC/$directory/." "$DEST/$directory/"
done

echo "OK: website/ → docs/ (HTML, CSS, JS y capturas)"
