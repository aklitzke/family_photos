#!/usr/bin/env python3
"""
Apply rotation suggestions from a CSV to data/history.toml.

Reads a CSV produced by detect_rotation.py and writes rotation = N into
the matching [[images]] blocks in history.toml.

Rules:
  - Never overwrites a rotation already set in history.toml (user input wins)
  - Only applies nonzero rotations (rotation=0 means no change needed)
  - Requires confidence >= 0.88 for orientation model results
  - Requires confidence >= 0.5 for face detection results
  - Default is dry run; pass --apply to actually write

Usage:
  uv run apply_rotations.py rotation_suggestions.csv
  uv run apply_rotations.py rotation_suggestions.csv --apply
  uv run apply_rotations.py rotation_suggestions.csv --apply --min-confidence-orientation 0.95
"""

import argparse
import csv
import shutil
import sys
from pathlib import Path

HISTORY_TOML = Path(__file__).parent.parent / "data" / "history.toml"

DEFAULT_MIN_CONF_ORIENTATION = 0.88
DEFAULT_MIN_CONF_FACE = 0.5


def load_csv(
    csv_path: Path,
    min_conf_orientation: float,
    min_conf_face: float,
) -> dict[str, int]:
    """Returns {key: rotation} for nonzero rotations passing per-method thresholds."""
    rotations: dict[str, int] = {}
    skipped_zero = 0
    skipped_conf: list[str] = []

    with open(csv_path, newline="") as f:
        for row in csv.DictReader(f):
            rotation = int(row["suggested_rotation"])
            confidence = float(row["confidence"])
            method = row["method"]
            key = row["key"]

            if rotation == 0:
                skipped_zero += 1
                continue

            threshold = min_conf_orientation if method == "orientation" else min_conf_face
            if confidence < threshold:
                skipped_conf.append(
                    f"  LOW CONF  [{method}] {key}: "
                    f"rotation={rotation} confidence={confidence:.4f} < {threshold}"
                )
                continue

            rotations[key] = rotation

    if skipped_conf:
        print("Below confidence threshold:", file=sys.stderr)
        for msg in skipped_conf:
            print(msg, file=sys.stderr)

    print(
        f"\nCSV summary: {len(rotations)} to apply, "
        f"{skipped_zero} at 0° (no change needed), "
        f"{len(skipped_conf)} below threshold",
        file=sys.stderr,
    )
    return rotations


def apply_rotations(
    toml_path: Path,
    rotations: dict[str, int],
    write: bool,
) -> tuple[int, int, int]:
    """
    Apply rotations to history.toml.
    Never overwrites existing rotations — user-set values always win.
    Returns (applied, skipped_existing, not_found_count).
    """
    lines = toml_path.read_text().splitlines(keepends=True)
    output: list[str] = []
    applied = 0
    skipped_existing = 0
    not_found = set(rotations.keys())

    i = 0
    while i < len(lines):
        line = lines[i]
        output.append(line)

        stripped = line.strip()
        if stripped.startswith('key = "'):
            key = stripped.split('"')[1]
            if key in rotations:
                not_found.discard(key)
                new_rotation = rotations[key]
                next_i = i + 1
                next_stripped = lines[next_i].strip() if next_i < len(lines) else ""

                if next_stripped.startswith("rotation = "):
                    existing = int(next_stripped.split("=")[1].strip())
                    skipped_existing += 1
                    print(f"  SKIP    {key}: already rotation = {existing} (user set)")
                else:
                    output.append(f"rotation = {new_rotation}\n")
                    applied += 1
                    print(f"  ADD     {key}: rotation = {new_rotation}")

        i += 1

    if not_found:
        print(
            f"\nWarning: {len(not_found)} keys not found in history.toml:",
            file=sys.stderr,
        )
        for k in sorted(not_found)[:10]:
            print(f"  {k}", file=sys.stderr)
        if len(not_found) > 10:
            print(f"  ...and {len(not_found) - 10} more", file=sys.stderr)

    if write:
        if applied > 0:
            backup = toml_path.with_suffix(".toml.bak")
            shutil.copy2(toml_path, backup)
            toml_path.write_text("".join(output))
            print(f"\nBacked up original to {backup}", file=sys.stderr)
            print(f"Wrote {toml_path}", file=sys.stderr)
        else:
            print("\nNothing to write.", file=sys.stderr)
    else:
        print(
            f"\n[dry run] Would apply {applied} rotation(s). Pass --apply to write.",
            file=sys.stderr,
        )

    return applied, skipped_existing, len(not_found)


def main() -> None:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("csv", help="CSV produced by detect_rotation.py")
    parser.add_argument(
        "--apply", action="store_true",
        help="Actually write to history.toml (default: dry run)",
    )
    parser.add_argument(
        "--min-confidence-orientation", type=float, default=DEFAULT_MIN_CONF_ORIENTATION,
        help=f"Min confidence for orientation model results (default: {DEFAULT_MIN_CONF_ORIENTATION})",
    )
    parser.add_argument(
        "--min-confidence-face", type=float, default=DEFAULT_MIN_CONF_FACE,
        help=f"Min confidence for face detection results (default: {DEFAULT_MIN_CONF_FACE})",
    )
    parser.add_argument("--history-toml", default=str(HISTORY_TOML))
    args = parser.parse_args()

    toml_path = Path(args.history_toml)
    if not toml_path.exists():
        print(f"Error: {toml_path} not found", file=sys.stderr)
        sys.exit(1)

    rotations = load_csv(
        Path(args.csv),
        args.min_confidence_orientation,
        args.min_confidence_face,
    )
    if not rotations:
        print("Nothing to apply.", file=sys.stderr)
        return

    print(f"\nChanges{' (dry run)' if not args.apply else ''}:", file=sys.stderr)
    applied, skipped, not_found = apply_rotations(toml_path, rotations, args.apply)

    print(
        f"\nSummary: applied={applied}  skipped_existing={skipped}  not_found={not_found}",
        file=sys.stderr,
    )


if __name__ == "__main__":
    main()
