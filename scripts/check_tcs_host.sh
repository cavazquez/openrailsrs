#!/usr/bin/env bash
# Optional Linux acceptance: actual C# fixture consumed by the Rust simulator.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
TCS_DOTNET="${OPENRAILSRS_DOTNET:-dotnet}"
TCS_OUT="$ROOT/tmp/or-tcs-host"
export DOTNET_CLI_TELEMETRY_OPTOUT=1
export DOTNET_CLI_HOME="$ROOT/tmp/dotnet-home"
export NUGET_PACKAGES="$ROOT/tmp/nuget"
"$TCS_DOTNET" build tools/or-tcs-host -o "$TCS_OUT" --ignore-failed-sources
python3 scripts/test_tcs_host.py --dotnet "$TCS_DOTNET" --host "$TCS_OUT/OrTcsHost.dll"
cargo run --locked -p openrailsrs-sim --example tcs_host_smoke -- \
  "$TCS_DOTNET" "$TCS_OUT/OrTcsHost.dll" "$ROOT/docs/fixtures/tcs/MinimalTcs.cs"
