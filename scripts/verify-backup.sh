#!/bin/bash
# Checks real FortiWeb backups without changing them or printing their contents:
# unedited save is byte-identical, a one-line edit changes only that edit,
# and every protected section is unchanged.
#   scripts/verify-backup.sh <backup.conf>...
set -euo pipefail
cd "$(dirname "$0")/.."
cargo run -q --release -p editor-formats --example verify_backup -- "$@"
