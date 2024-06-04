# syntax=docker/dockerfile:1

# Use the specific Rust version for building the application
ARG RUST_VERSION=1.78.0
ARG APP_NAME=jivachat

################################################################################
# Create a stage for building the application.

FROM rust:${RUST_VERSION}-alpine AS build
ARG APP_NAME
WORKDIR /app

# Install host build dependencies.
RUN apk add --no-cache clang lld musl-dev git openssl-dev pkgconfig libgcc

ENV OPENSSL_DIR=/usr

# Build the application.
# Leverage cache mounts for dependencies and compiled outputs.
RUN --mount=type=bind,source=src,target=src \
    --mount=type=bind,source=Cargo.toml,target=Cargo.toml \
    --mount=type=bind,source=Cargo.lock,target=Cargo.lock \
    --mount=type=cache,target=/app/target/ \
    --mount=type=cache,target=/usr/local/cargo/git/db \
    --mount=type=cache,target=/usr/local/cargo/registry/ \
    RUSTFLAGS="-Ctarget-feature=-crt-static" cargo build --target x86_64-unknown-linux-musl --locked --release && \
    cp ./target/x86_64-unknown-linux-musl/release/$APP_NAME /bin/server

################################################################################
# Create a new stage for running the application with minimal dependencies.

FROM alpine:3.18 AS final

RUN apk add --no-cache libgcc

# Create a non-privileged user that the app will run under.
ARG UID=10001
RUN adduser \
    --disabled-password \
    --gecos "" \
    --home "/nonexistent" \
    --shell "/sbin/nologin" \
    --no-create-home \
    --uid "${UID}" \
    appuser
USER appuser

# Copy the executable from the "build" stage.
COPY --from=build /bin/server /bin/

# Expose the port that the application listens on.
EXPOSE 3030

# What the container should run when it is started.
CMD ["/bin/server"]
