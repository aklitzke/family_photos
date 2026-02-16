#!/usr/bin/env bash
set -euo pipefail

DATASET="storage_rz2/family_history"
SNAPSHOT_NAME="${DATASET}@$(date +%Y-%m-%d_%H%M%S)"

echo "Creating snapshot: $SNAPSHOT_NAME"
sudo zfs snapshot "$SNAPSHOT_NAME"
echo "Done."
