# Debian amd64 build environment for buildDebian.sh, started by buildLinux.sh.
# Rust is fixed when the image is built; refresh it with:
#   podman build --no-cache --platform linux/amd64 --tag rustitles-build --file linuxBuild.Containerfile .
FROM docker.io/library/debian:13-slim
ARG RUST_VERSION=stable
ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo PATH=/opt/cargo/bin:$PATH
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential ca-certificates curl dpkg-dev file libssl-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --default-toolchain "$RUST_VERSION"
RUN cargo install cargo-deb --version 3.7.0 --locked
