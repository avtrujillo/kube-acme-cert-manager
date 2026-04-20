# TODO: finish refactoring and test

ARG RUST_VERSION=1.93.1
ARG RUST_DEBIAN=trixie
ARG K8S_VERSION=v1_35

FROM rust:${RUST_VERSION}-slim-${RUST_DEBIAN} AS deps

ARG K8S_VERSION

RUN echo "deb http://security.debian.org/debian-security trixie-security main contrib non-free" > /etc/apt/sources.list \
    && apt-get update \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Copy workspace Cargo manifests and create stub sources to cache dependency compilation
COPY Cargo.toml Cargo.toml

RUN cargo build --features ${K8S_VERSION} --release --bin acme 2>/dev/null || true

COPY . .
RUN cargo build --features ${K8S_VERSION} --release --bin acme

# Runtime stage
FROM debian:${RUST_DEBIAN}-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/acme /usr/local/bin/acme

ENTRYPOINT ["acme"]
