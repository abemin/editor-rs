#!/bin/bash
# Checks how Fortinet configuration files are detected, colored and folded,
# without changing them or printing their contents (counts, line numbers and
# command keywords only).
#   scripts/check-cli.sh <config>...
set -euo pipefail
cd "$(dirname "$0")/.."
cargo run -q --release -p editor-formats --example check_cli -- "$@"
