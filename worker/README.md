# Cloudflare Workers Backend

This is the backend for the Family Photos app, deployed as a Cloudflare Worker with direct R2 access.

## Architecture

- **Runtime**: Cloudflare Workers (edge serverless)
- **Storage**: Cloudflare R2 (S3-compatible object storage)
- **Framework**: workers-rs (Rust for Cloudflare Workers)
- **Image Processing**: image crate for thumbnail generation

## Features

- Direct R2 integration using native Workers bindings (no AWS SDK needed)
- Automatic thumbnail generation and storage at `thumbs/{original_path}`
- Edge deployment for low latency worldwide
- Serverless - no servers to manage

## Prerequisites

1. **Cloudflare account** with Workers enabled
2. **wrangler CLI** installed:
   ```bash
   npm install -g wrangler
   ```
3. **Rust** with wasm32-unknown-unknown target:
   ```bash
   rustup target add wasm32-unknown-unknown
   ```
4. **worker-build** tool:
   ```bash
   cargo install worker-build
   ```

## Setup

### 1. Login to Cloudflare

```bash
wrangler login
```

### 2. Create R2 Bucket

```bash
wrangler r2 bucket create family-photos
```

### 3. Upload Your Images

Upload your 5 family photos to R2 at the paths specified in `src/lib.rs`:
- `photos/family1.jpg`
- `photos/family2.jpg`
- `photos/family3.jpg`
- `photos/family4.jpg`
- `photos/family5.jpg`

You can upload via wrangler:
```bash
wrangler r2 object put family-photos/photos/family1.jpg --file=/path/to/your/image1.jpg
wrangler r2 object put family-photos/photos/family2.jpg --file=/path/to/your/image2.jpg
# ... etc
```

Or use the Cloudflare dashboard R2 interface.

### 4. Update Image Paths (Optional)

If your images are at different paths, edit `src/lib.rs` lines 5-9 to match your R2 object keys.

## Development

### Local Development

```bash
wrangler dev
```

This starts a local development server at `http://localhost:8787`.

**Note**: Local development with R2 requires either:
- Using `--remote` flag to run on Cloudflare's edge
- Setting up local R2 emulation (not officially supported)

Recommended approach for R2 testing:
```bash
wrangler dev --remote
```

### Build

```bash
worker-build --release
```

## Deployment

### Deploy to Cloudflare

```bash
wrangler deploy
```

This will:
1. Build your Worker
2. Upload to Cloudflare's edge network
3. Return your Worker URL (e.g., `https://family-photos-worker.your-subdomain.workers.dev`)

### Configure Custom Domain (Optional)

In the Cloudflare dashboard:
1. Go to Workers & Pages
2. Select your worker
3. Go to Settings > Triggers
4. Add a custom domain

Or via wrangler.toml:
```toml
routes = [
  { pattern = "photos.yourdomain.com/*", zone_name = "yourdomain.com" }
]
```

## API Endpoints

All endpoints will be available at your Worker URL:

- `GET /api/health` - Health check
- `GET /api/images/list` - List all images
- `GET /api/images/thumbnail/:id` - Get thumbnail (auto-generates if needed)
- `GET /api/images/full/:id` - Get full resolution image

## Thumbnail Storage

Thumbnails are automatically generated on first request and stored in R2 at:
```
thumbs/photos/family1.jpg
thumbs/photos/family2.jpg
...
```

This follows the same directory structure as your original images.

## Frontend Integration

After deploying, update your frontend to use the Worker URL:

1. Note your Worker URL from `wrangler deploy` output
2. Update frontend API calls to use this URL
3. The Worker includes CORS headers, so cross-origin requests will work

## Monitoring

View logs in real-time:
```bash
wrangler tail
```

Or view in Cloudflare dashboard:
- Workers & Pages > Your Worker > Logs

## Costs

Cloudflare Workers Free Tier includes:
- 100,000 requests/day
- 10ms CPU time per request

R2 Free Tier includes:
- 10 GB storage
- 1 million Class A operations/month (writes)
- 10 million Class B operations/month (reads)

Perfect for a personal family photo gallery!

## Troubleshooting

### "Bucket not found" error
Make sure you created the R2 bucket:
```bash
wrangler r2 bucket list
```

If not listed, create it:
```bash
wrangler r2 bucket create family-photos
```

### "Module not found" error during build
Install worker-build:
```bash
cargo install worker-build
```

### Images not loading
Check R2 object keys match the paths in `src/lib.rs`:
```bash
wrangler r2 object list family-photos
```

### Worker timeout on thumbnail generation
Large images may timeout. Consider:
- Reducing original image sizes
- Increasing Worker timeout limits (paid plan)
- Pre-generating thumbnails offline

## Migration from Axum Backend

The Worker provides the same API as the previous Axum backend, so no frontend changes are needed (except updating the base URL).

Key differences:
- Runs on Cloudflare's edge instead of your server
- No AWS credentials needed (uses Workers R2 bindings)
- Deployed globally for lower latency
- Serverless architecture

## License

MIT
