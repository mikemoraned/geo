# Pin the builder to the same Debian release as the runtime (bookworm) so the
# binary's glibc matches. Untagged `rust:slim` tracks the latest Debian stable,
# which rolled to trixie (glibc 2.38) and broke the bookworm-slim runtime.
FROM rust:slim-bookworm AS builder

# Git hash of the deployed source, passed by `just deploy` (`--build-arg`). The
# build script re-emits it as BUILD_GIT_HASH for env!() in the binary.
ARG BUILD_GIT_HASH=unknown
ENV BUILD_GIT_HASH=$BUILD_GIT_HASH

WORKDIR /prod
COPY . .
RUN cargo build --release -p server

FROM debian:bookworm-slim AS runner

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /prod/target/release/server /usr/local/bin/server
COPY crates/server/static /app/static

ENV LOOKOUT_STATIC_DIR=/app/static
ENV PORT=8080
EXPOSE 8080

CMD ["server"]
