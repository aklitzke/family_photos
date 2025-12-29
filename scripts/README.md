# Scripts

Local Rust scripts for managing R2 buckets and data files.

## Setup

1. **Copy the environment template:**
   ```bash
   cp .env.example .env
   ```

2. **Add your R2 credentials to `.env`:**
   - Get credentials from: https://dash.cloudflare.com → R2 → Manage R2 API Tokens
   - Fill in `CLOUDFLARE_ACCOUNT_ID`, `R2_ACCESS_KEY_ID`, and `R2_SECRET_ACCESS_KEY`

## Available Scripts

### `list_r2_files`

Lists all files in the `google_drive_pics` R2 bucket (as defined in `worker/wrangler.toml`).

**Usage:**
```bash
cd scripts
cargo run --bin list_r2_files
```

**Output:**
```
📦 Listing files in R2 bucket: google-drive-pics
  📄 Family History/Pile 1/2025-12-23-21-51-0001.jpg (2458392 bytes)
  📄 Family History/Pile 1/2025-12-23-21-51-0002.jpg (1893472 bytes)
  ...

✅ Total files: 5
```

## Adding New Scripts

Create new binaries in `src/bin/`:
```bash
touch src/bin/my_script.rs
cargo run --bin my_script
```

All scripts have access to:
- R2 buckets via S3-compatible API
- Local filesystem (read/write `../data/history.toml`, etc.)
- Shared types from `common` crate
