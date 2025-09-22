# Use a lightweight Debian image
FROM debian:bookworm-slim

# Install dependencies
RUN apt-get update && apt-get install -y \
    curl \
    build-essential \
    pkg-config \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Install Rust using rustup
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

# Set the working directory inside the container
WORKDIR /app

# Copy the entire project into the container
COPY . .

# Build the Rust application in release mode
RUN cargo build --release

# Expose the port your application will run on (adjust based on your app)
EXPOSE 8080

# Run the compiled binary
CMD ["./target/release/tauri-update-server"]

