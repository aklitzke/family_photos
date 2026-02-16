#!/usr/bin/env bash
set -euo pipefail

FAMILY_HISTORY_DIR="/mnt/storage_rz2/family_history"

if [ ! -d "$FAMILY_HISTORY_DIR" ]; then
  echo "Error: $FAMILY_HISTORY_DIR does not exist"
  exit 1
fi

echo "Making all files immutable (+i)..."
sudo find "$FAMILY_HISTORY_DIR" -type f -exec chattr +i {} +

echo "Making all directories append-only (+a)..."
sudo find "$FAMILY_HISTORY_DIR" -type d -exec chattr +a {} +

echo "Done. Files: immutable, Directories: append-only."
