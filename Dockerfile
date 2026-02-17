# Stage 1: Build the server binary
FROM rust:1.93-bookworm AS server-builder
WORKDIR /app
COPY common/ common/
COPY worker/ worker/
RUN cd worker && cargo build --release

# Stage 2: Runtime
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=server-builder /app/worker/target/release/family-photos /usr/local/bin/family-photos

ENV DATA_PATH=/srv/data
ENV IMAGES_PATH=/srv/images
ENV THUMBS_PATH=/srv/thumbs
ENV FRONTEND_PATH=/srv/frontend
ENV PORT=8082
VOLUME ["/srv/data", "/srv/images", "/srv/thumbs", "/srv/frontend"]

EXPOSE 8082
CMD ["family-photos"]
