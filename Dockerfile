# Use Debian as the base image for both build and runtime stages
FROM debian:buster-slim as base

# Set up necessary dependencies
RUN apt-get update && apt-get install -y curl build-essential pkg-config libssl-dev ca-certificates && rm -rf /var/lib/apt/lists/*

# Use the Rust image to build the project
FROM base as builder

# Install Rust using rustup
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y
ENV PATH=/root/.cargo/bin:$PATH

# Set the working directory inside the container
WORKDIR /app

# Copy the Cargo.toml and Cargo.lock files first to leverage Docker cache for dependencies
COPY Cargo.toml Cargo.lock ./

# Create a dummy main.rs to trigger dependency caching
RUN mkdir src && echo "fn main() {}" > src/main.rs

# Build dependencies (this step is cached unless dependencies change)
RUN cargo build --release || true

# Now copy the actual source code
COPY . .

# Build the application
RUN cargo build --release

# Use the same base image for the runtime to avoid GLIBC version mismatches
FROM base

# Set the working directory inside the container
WORKDIR /app

# Copy the built application from the builder stage
COPY --from=builder /app/target/release/tauri-update-server .

# Set environment variables if needed (e.g., these can also be set using --env-file)
ENV ADDRESS=0.0.0.0 \
    PORT=8080

# Expose the port the app runs on
EXPOSE 8080

# Run the binary
CMD ["./tauri-update-server"]

