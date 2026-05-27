#!/usr/bin/env bash
# Respalda CSV de velocidad OR existentes antes de una nueva captura.
#
# Uso (source desde scripts/capture_*):
#   source "$(dirname "$0")/lib/backup_or_speed_csv.sh"
#   backup_or_speed_csv "$ROAM" "Open Rails_explorerSpeed*.csv"

backup_or_speed_csv() {
  local roam="${1:?roam dir required}"
  local pattern="${2:?glob pattern required}"
  local ts moved=0 f dest

  if [[ ! -d "$roam" ]]; then
    return 0
  fi

  ts="$(date +%Y%m%d%H%M%S)"
  while IFS= read -r -d '' f; do
    dest="${f}.bak.${ts}"
    mv "$f" "$dest"
    echo "Backup: $(basename "$f") → $(basename "$dest")"
    moved=1
  done < <(
    find "$roam" -maxdepth 1 -type f -name "$pattern" ! -name '*.bak.*' -print0 2>/dev/null
  )

  if (( moved )); then
    echo "Capturas OR anteriores respaldadas (sufijo .bak.${ts})."
  else
    echo "Sin CSV previos que respaldar en $roam ($pattern)."
  fi
}
