# Family Photos

## Getting Started

### 1. Build the Frontend

```bash
cd frontend
trunk build --release
```

This compiles the Rust code to WebAssembly and creates an optimized build in `frontend/dist/`.

### 2. Run the Backend Server

From the project root:

```bash
cd backend
cargo run
```

The server will start on `http://localhost:3000`.

### 3. Open in Browser

Visit `http://localhost:3000` to see the application!

## Development Workflow

### Frontend Development with Live Reload

For development with automatic rebuilding:

```bash
cd frontend
trunk serve
```

This starts a development server on `http://localhost:8080` with hot reload.

**Note**: When using `trunk serve`, the backend API won't be available unless you also run the backend server separately.

### Full Stack Development

For the best development experience:

1. **Terminal 1** - Frontend with auto-rebuild:
   ```bash
   cd frontend
   trunk serve --open
   ```

2. **Terminal 2** - Backend API server:
   ```bash
   cd backend
   cargo run
   ```

3. **Access**: http://localhost:3000 (backend serves the frontend)

### Making Changes

- **Frontend**: Edit `frontend/src/lib.rs`, trunk will auto-rebuild
- **Backend**: Edit `backend/src/main.rs`, restart with `cargo run`

## API Endpoints

- `GET /api/health` - Health check endpoint
  - Returns: `{"status": "ok", "message": "Server is running"}`

## Building for Production

1. Build the frontend:
   ```bash
   cd frontend
   trunk build --release
   ```

2. Build the backend:
   ```bash
   cd backend
   cargo build --release
   ```

3. Run the production server:
   ```bash
   ./backend/target/release/backend
   ```

4. Deploy the binary - everything is self-contained!

## Troubleshooting

### WASM Build Fails
- Ensure you have the wasm32 target: `rustup target add wasm32-unknown-unknown`
- Update Rust: `rustup update stable`

### Frontend Not Loading
- Check that `frontend/dist/` exists and contains files
- Verify backend is serving from correct path
- Check browser console for errors

### Trunk Not Found
- Install trunk: `cargo install trunk`
- Ensure `~/.cargo/bin` is in your PATH

## License

This project is open source and available under the MIT License.
