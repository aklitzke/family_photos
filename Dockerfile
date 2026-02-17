# Stage 1: Build the server binary
FROM rust:1.93-bookworm AS server-builder
WORKDIR /app
COPY common/ common/
COPY worker/ worker/
RUN cd worker && cargo build --release

# Stage 2: Build the frontend WASM
FROM rust:1.93-bookworm AS frontend-builder
RUN rustup target add wasm32-unknown-unknown
RUN cargo install trunk --locked
WORKDIR /app
COPY common/ common/
COPY frontend/ frontend/
RUN cd frontend && trunk build --release

# Stage 3: Runtime
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=server-builder /app/worker/target/release/family-photos /usr/local/bin/family-photos
COPY --from=frontend-builder /app/frontend/dist /srv/frontend
COPY data/ /srv/data/

ENV DATA_PATH=/srv/data
ENV THUMBS_PATH=/srv/thumbs
ENV FRONTEND_PATH=/srv/frontend
ENV PORT=8082

EXPOSE 8082
CMD ["family-photos"]
