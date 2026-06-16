#!/usr/bin/env bash
# Helpers compartidos para lanzar openrailsrs-render3d contra rutas OR/MSTS externas.
set -euo pipefail

render3d_repo_root() {
  local here
  here="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
  printf '%s\n' "$here"
}

# Fish (y otros shells) pueden dejar "~" literal en variables de entorno.
render3d_expand_path() {
  local p="$1"
  case "$p" in
    "~") printf '%s\n' "$HOME" ;;
    "~/"*) printf '%s\n' "${HOME}/${p:2}" ;;
    *) printf '%s\n' "$p" ;;
  esac
}

render3d_first_existing_dir() {
  local p expanded
  for p in "$@"; do
    expanded="$(render3d_expand_path "$p")"
    if [[ -n "$expanded" && -d "$expanded" ]]; then
      printf '%s\n' "$expanded"
      return 0
    fi
  done
  return 1
}

render3d_routes_parent() {
  local root="$1"
  render3d_first_existing_dir \
    "$root/ROUTES" \
    "$root/Routes" \
    "$root/routes"
}

render3d_resolve_msts_root() {
  local explicit="${1:-}"
  if [[ -n "$explicit" ]]; then
    explicit="$(render3d_expand_path "$explicit")"
    if [[ ! -d "$explicit/GLOBAL" && ! -d "$explicit/global" ]]; then
      echo "error: $explicit no parece una raiz MSTS/OR (falta GLOBAL/)" >&2
      if [[ "$1" == "~"* ]]; then
        echo "tip: en fish usa \$HOME/routes/... en vez de ~/routes/... (el ~ no se expande en export)" >&2
      fi
      return 1
    fi
    printf '%s\n' "$explicit"
    return 0
  fi
  return 1
}

render3d_find_route_dir() {
  local msts_root="$1"
  shift
  local routes_parent
  routes_parent="$(render3d_routes_parent "$msts_root")" || {
    echo "error: no hay carpeta ROUTES/ en $msts_root" >&2
    return 1
  }

  local name
  for name in "$@"; do
    if [[ -d "$routes_parent/$name" ]]; then
      printf '%s\n' "$routes_parent/$name"
      return 0
    fi
  done

  local candidate best_count=-1 best_dir=""
  for candidate in "$routes_parent"/*; do
    [[ -d "$candidate" ]] || continue
    local tdb_count=0
    local tdb
    shopt -s nullglob
    for tdb in "$candidate"/*.tdb "$candidate"/*.TDB; do
      tdb_count=$((tdb_count + 1))
    done
    shopt -u nullglob
    [[ "$tdb_count" -gt 0 ]] || continue
    if [[ "$tdb_count" -gt "$best_count" ]]; then
      best_count=$tdb_count
      best_dir=$candidate
    fi
  done

  if [[ -n "$best_dir" ]]; then
    printf '%s\n' "$best_dir"
    return 0
  fi

  echo "error: no se encontro ninguna ruta bajo $routes_parent (probados: $*)" >&2
  return 1
}

render3d_validate_route_dir() {
  local route_dir="$1"
  if ! render3d_first_existing_dir \
    "$route_dir/TILES" "$route_dir/Tiles" "$route_dir/tiles" >/dev/null; then
    echo "error: $route_dir no tiene TILES/ (instalacion incompleta?)" >&2
    return 1
  fi
  if ! render3d_first_existing_dir \
    "$route_dir/WORLD" "$route_dir/world" >/dev/null; then
    echo "warn: $route_dir no tiene WORLD/ — el tile central puede fallar" >&2
  fi
}

render3d_usage_header() {
  cat <<EOF_USAGE
Uso: $1 [opciones extra de cargo/render3d]

Variables de entorno:
  ${2}
  RENDER3D_RELEASE=1     compilar en --release (default: 1)
  OPENRAILSRS_TEXTURE_DEBUG=1   log de resolucion de texturas al salir

Opciones extra pasadas al binario (despues de --):
  --radius 0|1|2   tiles alrededor del centro (default depende del script)
  --tile-x N --tile-z N   tile concreto (si no, centroide de WORLD/)
  --activity ACT.act   actividad MSTS (spawn + estacion/hora)
  --player-path PATHS/foo.pat   path del jugador sin .act (OR-only)
  --path-offset-m N    metros desde inicio del .pat
  --no-hud             ocultar overlay de depuracion (F3 alterna en runtime)
EOF_USAGE
}

render3d_run() {
  local msts_root route_dir
  msts_root="$(render3d_expand_path "$1")"
  route_dir="$(render3d_expand_path "$2")"
  shift 2

  render3d_validate_route_dir "$route_dir"

  local repo
  repo="$(render3d_repo_root)"
  cd "$repo"

  local -a cargo=(cargo run)
  if [[ "${RENDER3D_RELEASE:-1}" == "1" ]]; then
    cargo=(cargo run --release)
  fi

  echo "=== openrailsrs-render3d ==="
  echo "msts_root: $msts_root"
  echo "route:     $route_dir"
  echo "repo:      $repo"
  echo ""

  exec "${cargo[@]}" -p openrailsrs-render3d -- \
    --route "$route_dir" \
    --msts-root "$msts_root" \
    "$@"
}
