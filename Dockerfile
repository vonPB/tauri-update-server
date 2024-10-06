# Stage 1: Build Stage (Rust with musl support)
FROM rust:1.81 as builder

# Install musl and necessary dependencies
RUN apt-get update && apt-get install -y musl-tools && rustup target add x86_64-unknown-linux-musl

# Set the working directory inside the container
WORKDIR /usr/src/tauri-update-server

# Copy the Cargo files
COPY Cargo.toml Cargo.lock ./

# Create a dummy main.rs to pre-build dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs

# Build dependencies with musl
RUN cargo build --release --target x86_64-unknown-linux-musl || true

# Now copy the actual source code and build the application with musl
COPY . .
RUN cargo build --release --target x86_64-unknown-linux-musl

# Stage 2: Use a minimal runtime (Alpine or Distroless)
FROM scratch

# Set the working directory inside the container
WORKDIR /app

# Copy the compiled musl binary from the build stage
COPY --from=builder /usr/src/tauri-update-server/target/x86_64-unknown-linux-musl/release/tauri-update-server .

# Expose the port that the Actix Web app will run on
EXPOSE 8080

# Command to run the application
CMD ["./tauri-update-server"]

