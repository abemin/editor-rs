#!/bin/bash
# Measures the speed targets on a release build.
#   scripts/perf-report.sh [--backup <path to a real FortiWeb backup>]
# Writes target/perf-report.md and exits non-zero if any target is missed.
set -euo pipefail
cd "$(dirname "$0")/.."
backup=""
if [ "${1:-}" = "--backup" ]; then backup="$2"; fi
scripts/bundle.sh release
app="$(scripts/bundle.sh release --print-path)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
python3 -I scripts/perf_report.py "$app" "$work" "$backup" target/perf-report.md
