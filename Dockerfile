# Use a multi-stage build with Rust official image
FROM rust:1.75-slim-buster as builder

# Create a new empty shell project
RUN USER=root cargo new --bin tauri-update-server
WORKDIR /tauri-update-server

# Copy manifests first (better layer caching)
COPY Cargo.lock Cargo.toml ./

# Build dependencies - this is the caching Docker layer
RUN cargo build --release
RUN rm src/*.rs

# Copy actual source code
COPY src src/

# Build for release
RUN rm ./target/release/deps/tauri_update_server*
RUN cargo build --release

# Final stage: Create minimal runtime image
FROM debian:buster-slim

# Install only runtime dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy the built binary from builder
COPY --from=builder /tauri-update-server/target/release/tauri-update-server /usr/local/bin/

# Create non-root user
RUN useradd -ms /bin/bash appuser
USER appuser

# Configure the port
EXPOSE 8080

# Run the binary
CMD ["tauri-update-server"]
