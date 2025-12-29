# Family Photos

A modern web application for storing and organizing family photos, built with a full Rust stack: Axum backend and egui frontend compiled to WebAssembly.

## Tech Stack

- **Backend**: Rust with Axum web framework
- **Frontend**: Rust with egui (compiled to WASM)
- **Architecture**: Full Rust stack with canvas-based rendering

## Prerequisites

Before you begin, ensure you have the following installed:

- [Rust](https://rustup.rs/) (latest stable version - 1.88+)
- [Trunk](https://trunkrs.dev/) - WASM web application bundler
  ```bash
  cargo install trunk
  ```
- WASM target for Rust:
  ```bash
  rustup target add wasm32-unknown-unknown
  ```

## Project Structure

```
family_photos/
├── backend/              # Rust/Axum server
│   ├── src/
│   │   └── main.rs      # Server implementation
│   └── Cargo.toml       # Backend dependencies
├── frontend/            # egui + WASM frontend
│   ├── src/
│   │   └── lib.rs       # egui app implementation
│   ├── Cargo.toml       # Frontend dependencies
│   ├── Trunk.toml       # Trunk build configuration
│   ├── index.html       # HTML shell
│   └── dist/            # Built WASM output (generated)
└── README.md
```

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

## Features

- **Canvas-Based Rendering**: egui renders directly to HTML5 canvas, similar to Flutter
- **Full Rust Stack**: Type safety from frontend to backend
- **Immediate Mode GUI**: Simple, reactive UI programming model
- **WASM**: Fast, secure frontend with near-native performance
- **Single Server**: Backend serves both API and frontend
- **Cross-Platform**: Can compile to desktop/mobile later

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

## UI Features

The landing page includes:
- Large title and description
- Interactive buttons (Get Started, Learn More, Check Health)
- Feature showcase with icons
- Health check API integration example
- Clean, centered layout
- Responsive design

## Why egui?

- **Flutter-like**: Canvas-based rendering instead of HTML/CSS
- **Immediate Mode**: Simple mental model, easy to reason about
- **Performance**: Compiled to WASM, runs at near-native speed
- **Full Rust**: Share code between frontend and backend
- **Portable**: Same code can run on web, desktop, and mobile

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
