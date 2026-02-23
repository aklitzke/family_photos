#!/usr/bin/env python3
"""
Detect required rotation for family photos.

Two-stage pipeline:

  Stage 1 — Face detection (insightface buffalo_l, local GPU):
    Tries all 4 rotations per image. Face detectors are trained on upright
    faces, so the rotation with the highest detection confidence is almost
    certainly correct. Fast and very accurate for portraits / group shots.
    Only high-confidence results (margin >= FACE_HIGH_CONF_THRESHOLD) are
    accepted; lower-confidence detections fall through to Stage 2.

  Stage 2 — EfficientNetV2 orientation classifier (local GPU):
    Fine-tuned EfficientNetV2-S from DuarteBarbosa/deep-image-orientation-detection
    on HuggingFace. Trained on 189K images (COCO + Kaggle), 98.82% accuracy.
    4-class classifier: predicts which of 0/90/180/270 degrees to apply.
    Model (~82 MB ONNX) is downloaded on first run and cached in ml/models/.

Output CSV columns:
  path               - absolute path to image file
  key                - history.toml key (relative path, no extension)
  suggested_rotation - degrees to apply (0 / 90 / 180 / 270)
  confidence         - face detection margin, or softmax probability for orientation
  method             - "face" or "orientation"
  notes              - face count, top-2 class probs, or failure reason

Usage:
  uv run detect_rotation.py                        # full run
  uv run detect_rotation.py --stage faces          # face detection only
  uv run detect_rotation.py --stage orientation    # orientation model only
  uv run detect_rotation.py --limit 20             # quick test
  uv run detect_rotation.py --only-nonzero         # only rows where rotation != 0
  uv run detect_rotation.py --help
"""

import argparse
import csv
import sys
import warnings
from pathlib import Path

warnings.filterwarnings("ignore")

import cv2
import numpy as np
import onnxruntime as ort
import torch
from huggingface_hub import hf_hub_download
from insightface.app import FaceAnalysis
from PIL import Image
from torchvision import transforms as T
from tqdm import tqdm

IMAGES_DIR = Path(__file__).parent.parent.parent / "family_photos" / "images" / "15mb_max"
HISTORY_TOML = Path(__file__).parent.parent / "data" / "history.toml"
MODELS_DIR = Path(__file__).parent / "models"
IMAGE_EXTENSIONS = {".jpg", ".jpeg", ".png", ".tif", ".tiff", ".bmp", ".webp"}
ROTATIONS = [0, 90, 180, 270]

FACE_CONF_THRESHOLD = 0.5
FACE_MARGIN_THRESHOLD = 0.15
FACE_HIGH_CONF_THRESHOLD = 0.5  # face margins >= this are trusted without orientation model

ORIENTATION_REPO = "DuarteBarbosa/deep-image-orientation-detection"
ORIENTATION_MODEL_FILE = "orientation_model_v2_0.9882.onnx"
# Class index → degrees to apply (from model config)
ORIENTATION_CLASS_MAP = {0: 0, 1: 90, 2: 180, 3: 270}
ORIENTATION_TRANSFORMS = T.Compose([
    T.Resize((384, 384)),
    T.ToTensor(),
    T.Normalize(mean=[0.485, 0.456, 0.406], std=[0.229, 0.224, 0.225]),
])


# ---------------------------------------------------------------------------
# Shared helpers
# ---------------------------------------------------------------------------

def setup_device() -> torch.device:
    if torch.cuda.is_available():
        device = torch.device("cuda")
        name = torch.cuda.get_device_name(0)
        vram = torch.cuda.get_device_properties(0).total_memory / 1024**3
        print(f"GPU: {name} ({vram:.1f} GB VRAM)", file=sys.stderr)
    else:
        device = torch.device("cpu")
        print("CUDA not available, using CPU", file=sys.stderr)
    return device


def rotate_pil(img: Image.Image, degrees: int) -> Image.Image:
    return img.rotate(-degrees, expand=True)


def parse_existing_rotations(history_toml: Path) -> dict[str, int]:
    rotations: dict[str, int] = {}
    current_key = None
    for line in history_toml.read_text().splitlines():
        line = line.strip()
        if line.startswith("key = "):
            current_key = line.split('"')[1]
        elif line.startswith("rotation = ") and current_key is not None:
            rotations[current_key] = int(line.split("=")[1].strip())
    return rotations


def key_from_path(image_path: Path, images_dir: Path) -> str:
    return str(image_path.relative_to(images_dir).with_suffix(""))


# ---------------------------------------------------------------------------
# Stage 1: face detection
# ---------------------------------------------------------------------------

def load_face_detector() -> FaceAnalysis:
    print("Loading insightface (buffalo_l, detection only)...", file=sys.stderr)
    app = FaceAnalysis(
        name="buffalo_l",
        allowed_modules=["detection"],
        providers=["CUDAExecutionProvider", "CPUExecutionProvider"],
    )
    app.prepare(ctx_id=0, det_size=(640, 640))
    return app


def face_detect_rotation(
    face_app: FaceAnalysis, img: Image.Image
) -> tuple[int | None, float, str]:
    """
    Returns (rotation, margin, notes) if a clear winner found,
    or (None, margin, reason) if ambiguous / no faces.
    """
    scores: dict[int, float] = {}
    counts: dict[int, int] = {}

    for rot in ROTATIONS:
        rotated = rotate_pil(img, rot)
        bgr = cv2.cvtColor(np.array(rotated), cv2.COLOR_RGB2BGR)
        faces = face_app.get(bgr)
        if faces:
            best = max(f.det_score for f in faces)
            if best >= FACE_CONF_THRESHOLD:
                scores[rot] = best
                counts[rot] = len(faces)

    if not scores:
        return None, 0.0, "no_faces"

    best_rot = max(scores, key=scores.__getitem__)
    best_score = scores[best_rot]
    others = [s for r, s in scores.items() if r != best_rot]
    margin = best_score - (max(others) if others else 0.0)

    if margin < FACE_MARGIN_THRESHOLD:
        return None, margin, "face_ambiguous"

    n = counts[best_rot]
    return best_rot, margin, f"{n}_face{'s' if n != 1 else ''}"


def run_face_stage(
    images: list[Path], images_dir: Path
) -> tuple[dict[str, dict], list[Path]]:
    """
    Run face detection on all images.
    Returns (resolved, unresolved_paths).
    """
    setup_device()
    face_app = load_face_detector()

    resolved: dict[str, dict] = {}
    unresolved: list[Path] = []

    print(f"\nStage 1: face detection on {len(images)} images", file=sys.stderr)
    for image_path in tqdm(images, unit="img", desc="faces", file=sys.stderr):
        key = key_from_path(image_path, images_dir)
        try:
            img = Image.open(image_path).convert("RGB")
        except Exception as e:
            tqdm.write(f"  error opening {image_path}: {e}", file=sys.stderr)
            continue

        rotation, confidence, notes = face_detect_rotation(face_app, img)
        if rotation is not None and confidence >= FACE_HIGH_CONF_THRESHOLD:
            resolved[key] = {
                "path": str(image_path),
                "rotation": rotation,
                "confidence": confidence,
                "method": "face",
                "notes": notes,
            }
        else:
            unresolved.append(image_path)

    resolved_n = len(resolved)
    unresolved_n = len(unresolved)
    total = resolved_n + unresolved_n
    print(
        f"Face stage done: {resolved_n} resolved ({resolved_n/total*100:.0f}%), "
        f"{unresolved_n} need orientation model",
        file=sys.stderr,
    )
    return resolved, unresolved


# ---------------------------------------------------------------------------
# Stage 2: EfficientNetV2 orientation classifier
# ---------------------------------------------------------------------------

def load_orientation_model() -> ort.InferenceSession:
    MODELS_DIR.mkdir(exist_ok=True)
    model_path = MODELS_DIR / ORIENTATION_MODEL_FILE
    if not model_path.exists():
        print(f"Downloading orientation model from HuggingFace ({ORIENTATION_REPO})...", file=sys.stderr)
        hf_hub_download(
            repo_id=ORIENTATION_REPO,
            filename=ORIENTATION_MODEL_FILE,
            local_dir=str(MODELS_DIR),
        )
        print("Download complete.", file=sys.stderr)
    else:
        print(f"Using cached orientation model: {model_path}", file=sys.stderr)

    session = ort.InferenceSession(
        str(model_path),
        providers=["CUDAExecutionProvider", "CPUExecutionProvider"],
    )
    provider = session.get_providers()[0]
    print(f"Orientation model loaded (provider: {provider})", file=sys.stderr)
    return session


def orientation_detect_rotation(
    session: ort.InferenceSession, img: Image.Image
) -> tuple[int, float, str]:
    """
    Run EfficientNetV2 orientation classifier on img.
    Returns (rotation, confidence, notes).
    confidence is the softmax probability of the top class.
    notes includes the top-2 predictions for transparency.
    """
    input_tensor = ORIENTATION_TRANSFORMS(img).unsqueeze(0).numpy()
    input_name = session.get_inputs()[0].name
    logits = session.run(None, {input_name: input_tensor})[0][0]  # shape: (4,)

    exp = np.exp(logits - logits.max())
    probs = exp / exp.sum()

    top2_idx = np.argsort(probs)[::-1][:2]
    top2 = [(ORIENTATION_CLASS_MAP[i], float(probs[i])) for i in top2_idx]

    predicted_idx = int(top2_idx[0])
    rotation = ORIENTATION_CLASS_MAP[predicted_idx]
    confidence = float(probs[predicted_idx])
    notes = f"{top2[0][0]}deg:{top2[0][1]:.3f},{top2[1][0]}deg:{top2[1][1]:.3f}"

    return rotation, confidence, notes


def run_orientation_stage(
    images: list[Path], images_dir: Path
) -> dict[str, dict]:
    """Run EfficientNetV2 orientation model on the given images. Returns resolved dict."""
    session = load_orientation_model()
    resolved: dict[str, dict] = {}

    print(f"\nStage 2: orientation model on {len(images)} images", file=sys.stderr)
    for image_path in tqdm(images, unit="img", desc="orientation", file=sys.stderr):
        key = key_from_path(image_path, images_dir)
        try:
            img = Image.open(image_path).convert("RGB")
        except Exception as e:
            tqdm.write(f"  error opening {image_path}: {e}", file=sys.stderr)
            continue

        rotation, confidence, notes = orientation_detect_rotation(session, img)
        resolved[key] = {
            "path": str(image_path),
            "rotation": rotation,
            "confidence": confidence,
            "method": "orientation",
            "notes": notes,
        }

    return resolved


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def write_csv(results: dict[str, dict], output_path: Path, only_nonzero: bool) -> int:
    written = 0
    with open(output_path, "w", newline="") as f:
        writer = csv.writer(f)
        writer.writerow(["path", "key", "suggested_rotation", "confidence", "method", "notes"])
        for key, r in results.items():
            if only_nonzero and r["rotation"] == 0:
                continue
            writer.writerow([
                r["path"], key, r["rotation"],
                f"{r['confidence']:.4f}", r["method"], r["notes"],
            ])
            written += 1
    return written


def main() -> None:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--output", "-o", default="rotation_suggestions.csv")
    parser.add_argument(
        "--stage", choices=["faces", "orientation", "both"], default="both",
        help="Which stage to run (default: both)",
    )
    parser.add_argument("--skip-existing", action="store_true", default=True)
    parser.add_argument("--no-skip-existing", dest="skip_existing", action="store_false")
    parser.add_argument("--only-nonzero", action="store_true", default=False)
    parser.add_argument("--images-dir", default=str(IMAGES_DIR))
    parser.add_argument("--limit", type=int, default=None)
    args = parser.parse_args()

    images_dir = Path(args.images_dir)
    if not images_dir.exists():
        print(f"Error: images directory not found: {images_dir}", file=sys.stderr)
        sys.exit(1)

    all_images = sorted(
        p for p in images_dir.rglob("*") if p.suffix.lower() in IMAGE_EXTENSIONS
    )
    print(f"Found {len(all_images)} images in {images_dir}", file=sys.stderr)

    existing = parse_existing_rotations(HISTORY_TOML) if HISTORY_TOML.exists() else {}
    if args.skip_existing and existing:
        to_process = [p for p in all_images if key_from_path(p, images_dir) not in existing]
        skipped = len(all_images) - len(to_process)
        if skipped:
            print(f"Skipping {skipped} already set in history.toml", file=sys.stderr)
    else:
        to_process = all_images

    if args.limit:
        to_process = to_process[: args.limit]

    print(f"Processing {len(to_process)} images", file=sys.stderr)

    all_results: dict[str, dict] = {}

    if args.stage in ("faces", "both"):
        face_results, unresolved = run_face_stage(to_process, images_dir)
        all_results.update(face_results)
    else:
        unresolved = to_process

    if args.stage in ("orientation", "both"):
        orientation_results = run_orientation_stage(unresolved, images_dir)
        all_results.update(orientation_results)

    output_path = Path(args.output)
    written = write_csv(all_results, output_path, args.only_nonzero)

    face_n = sum(1 for r in all_results.values() if r["method"] == "face")
    orientation_n = sum(1 for r in all_results.values() if r["method"] == "orientation")
    print(
        f"\nWrote {written} rows to {output_path}\n"
        f"face={face_n}  orientation={orientation_n}",
        file=sys.stderr,
    )


if __name__ == "__main__":
    main()
