# Family Photos

## Prerequisites

Before you begin, ensure you have the following installed:

- [Rust](https://rustup.rs/) (latest stable version)
- [Node.js](https://nodejs.org/) (v20.19.0 or higher recommended)
- npm (comes with Node.js)

## Getting Started

### 1. Install Frontend Dependencies

```bash
cd frontend
npm install
```

### 2. Build the Frontend

```bash
npm run build
```

This creates an optimized production build in `frontend/dist/`.

### 3. Run the Backend Server

From the project root:

```bash
cd backend
cargo run
```

The server will start on `http://localhost:3000`.

## Development Workflow

### Frontend Development

To work on the frontend with hot module replacement (HMR):

```bash
cd frontend
npm run dev
```

This starts the Vite dev server on `http://localhost:5173`.

When ready to test with the backend:

```bash
npm run build
```

### Backend Development

The backend serves the built frontend from `frontend/dist/`:

```bash
cd backend
cargo run
```

Changes to Rust code require restarting the server.

## API Endpoints

- `GET /api/health` - Health check endpoint
  - Returns: `{"status": "ok", "message": "Server is running"}`

## Building for Production

1. Build the frontend:
   ```bash
   cd frontend
   npm run build
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

## License

This project is open source and available under the MIT License.
