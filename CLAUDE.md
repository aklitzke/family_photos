# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Family Photos is a full-stack application for viewing and managing family photos stored in Cloudflare R2. The project consists of:
- **Worker**: Cloudflare Workers backend that serves images and metadata
- **Frontend**: Rust WASM frontend using egui for the UI
- **Common**: Shared types between worker and frontend
- **Scripts**: Utility scripts for R2 operations and thumbnail generation

## Architecture

### Multi-Crate Workspace Structure

This is a Cargo workspace with 4 crates:

1. **`worker/`** - Cloudflare Worker (backend API)
   - Uses `worker-rs` to create a Rust-based Cloudflare Worker
   - Serves REST API endpoints under `/api/*`
   - Handles thumbnail generation on-the-fly using the `image` crate
   - Fetches image metadata from `data/history.toml` (bundled in debug, fetched from GitHub API in release)
   - Implements AWS SigV4 signing in `sigv4.rs` for R2 presigned URLs
   - Connects to two R2 buckets: `google_drive_pics` (source images) and `thumbnails` (cached thumbnails)

2. **`frontend/`** - Web UI (Rust WASM using egui)
   - Compiled to WASM using `trunk serve` for development
   - Uses `eframe` for the UI framework (immediate mode GUI)
   - Implements three pages: Images, Artifacts, and Health
   - Uses batch thumbnail loading to optimize network requests
   - Supports image zoom with scroll-to-zoom and pan functionality
   - Handles image rotation based on metadata from `history.toml`

3. **`common/`** - Shared types
   - Contains all API request/response types (e.g., `ImageMetadata`, `Artifact`, `HistoryData`)
   - Used by both worker and frontend to ensure type consistency

4. **`scripts/`** - Admin utilities
   - `add_images_to_history.rs`: Scans a directory and adds image entries to `history.toml`
   - `generate_artifacts.rs`: Creates artifact entries from FastFoto filename patterns
   - `generate_thumbnails.rs`: Pre-generates thumbnails for R2 images locally
   - `list_r2_files.rs`: Lists files in R2 buckets
   - Uses AWS SDK for S3-compatible operations with Cloudflare R2

### Data Flow

1. **Image Metadata Source**: `data/history.toml` contains:
   - List of images with their R2 keys and optional rotation metadata
   - List of artifacts with `front1` (required), `front2` (optional), and `back1` (optional) images

2. **Development vs Production**:
   - **Debug builds**: Bundle `history.toml` directly into the worker binary
   - **Release builds**: Fetch `history.toml` from GitHub API at runtime

3. **Thumbnail Strategy**:
   - Frontend requests thumbnails in batches via POST `/api/images/thumbnails`
   - Worker checks `thumbnails` R2 bucket first (cache)
   - If not cached, worker generates on-the-fly from source and stores in cache
   - All thumbnails are JPEG with 300px width

4. **Full Image Access**:
   - Frontend requests presigned URL via GET `/api/images/full?key=<key>`
   - Worker generates AWS SigV4 presigned URL with 1-hour expiry
   - Frontend fetches image directly from R2 using presigned URL

## Common Development Commands

### Worker (Backend)

```bash
cd worker

# Development with remote R2 (recommended)
npx wrangler dev --remote

# Deploy to production
npx wrangler deploy
```

The worker runs on `http://localhost:8787` in development.

### Frontend (UI)

```bash
cd frontend

# Development server with hot reload
trunk serve

# Build release WASM
trunk build --release
```

The frontend runs on `http://localhost:8080` in development.

### Scripts (Admin Tools)

```bash
cd scripts

# Add all images from a directory to history.toml
# Run without arguments to see all options (base path stripping, prefix, etc.)
cargo run --bin add_images_to_history /path/to/images

# Generate artifacts from FastFoto image patterns
cargo run --bin generate_artifacts

# Generate thumbnails locally
cargo run --bin generate_thumbnails

# List R2 files
cargo run --bin list_r2_files
```

**Note**:
- `add_images_to_history` only requires filesystem access
- `generate_thumbnails` and `list_r2_files` require AWS credentials configured in environment variables for R2 access

### Testing

The app is tested manually by visiting `http://localhost:8080` with both worker and frontend running.

## Important Implementation Details

### Thumbnail File Naming

- Source images can be any format (`.jpg`, `.tif`, `.png`, etc.)
- Thumbnails are **always** stored as `.jpg` regardless of source format
- Thumbnail keys in R2 are prefixed with the source bucket binding name (e.g., `google_drive_pics/path/to/image.jpg`)

### Image Rotation

- `history.toml` can specify rotation in degrees: `rotation = 270`
- Frontend applies rotation when rendering images
- Valid rotation values: 0, 90, 180, 270

### SigV4 Presigned URL Generation

The worker implements custom AWS SigV4 signing (`worker/src/sigv4.rs`) because:
- Cloudflare Workers environment doesn't support standard AWS SDK
- R2 is S3-compatible and requires SigV4 authentication
- Custom implementation includes timestamp formatting without external time libraries

### API Endpoints

- `GET /api/health` - Health check
- `GET /api/images/list` - Returns all image metadata from `history.toml`
- `GET /api/artifacts/list` - Returns all artifact metadata from `history.toml`
- `GET /api/images/thumbnail?key=<key>` - Single thumbnail (legacy, prefer batch)
- `POST /api/images/thumbnails` - Batch thumbnail request (body: `{keys: [...]}`), returns map of key -> JPEG bytes
- `GET /api/images/full?key=<key>` - Returns presigned URL for full-resolution image

### Environment Variables (Worker)

Required for production (presigned URL generation):
- `CLOUDFLARE_ACCOUNT_ID`
- `R2_ACCESS_KEY_ID`
- `R2_SECRET_ACCESS_KEY`

Set these via `wrangler secret put <KEY>` or in the Cloudflare dashboard.

### Local Development Configuration

The frontend uses different API base URLs:
- **Debug**: `http://localhost:8787` (local worker)
- **Release**: Empty string (same origin as frontend)

The worker uses different history.toml loading:
- **Debug**: Bundled file via `include_str!`
- **Release**: Fetched from GitHub API

## Project-Specific Conventions

- All API responses use JSON with strongly-typed serde structs defined in `common/`
- Error handling uses `worker::Result` in the worker and `Result<T, String>` in frontend
- Frontend uses `AsyncResource<T>` wrapper to manage loading states uniformly
- Logging in worker uses `console_log!` macro
