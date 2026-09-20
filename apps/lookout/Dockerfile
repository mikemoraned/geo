# Pin the builder to the same Debian release as the runtime (bookworm) so the
# binary's glibc matches. Untagged `rust:slim` tracks the latest Debian stable,
# which rolled to trixie (glibc 2.38) and broke the bookworm-slim runtime.
FROM rust:slim-bookworm AS builder

# Git hash of the deployed source, passed by `just deploy` (`--build-arg`). The
# build script re-emits it as BUILD_GIT_HASH for env!() in the binary.
ARG BUILD_GIT_HASH=unknown
ENV BUILD_GIT_HASH=$BUILD_GIT_HASH

# wasm-pack, prebuilt: building it from source costs minutes of every deploy. Pinned to what
# `just prerequisites` installs, so a deploy compiles the core the same way a laptop does.
ARG WASM_PACK_VERSION=0.15.0
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && curl -sSfL "https://github.com/rustwasm/wasm-pack/releases/download/v${WASM_PACK_VERSION}/wasm-pack-v${WASM_PACK_VERSION}-$(uname -m)-unknown-linux-musl.tar.gz" \
       | tar xz -C /usr/local/bin --strip-components=1 --wildcards "*/wasm-pack" \
    && rustup target add wasm32-unknown-unknown

WORKDIR /prod
COPY . .
RUN cargo build --release -p server
# Into the static dir, which is then copied from the builder rather than from the context: the
# wasm is generated, so a checkout does not carry it.
RUN wasm-pack build crates/platform/web/bridge --target web --out-dir ../../../server/static/wasm --out-name lookout
# The crossings a browser fetches, beside it. Which version is `just deploy`'s to say, from
# the same `crossings.version` the device's build script reads, so a deploy and a board are
# never carrying different sets.
ARG CROSSINGS_VERSION
RUN test -n "$CROSSINGS_VERSION" || { echo "CROSSINGS_VERSION is required" >&2; exit 1; }; \
    cp "data/medallion/gold/artifact=crossings/version=$CROSSINGS_VERSION/crossings.json" \
        crates/server/static/crossings.json

FROM debian:bookworm-slim AS runner

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /prod/target/release/server /usr/local/bin/server
COPY --from=builder /prod/crates/server/static /app/static

ENV LOOKOUT_STATIC_DIR=/app/static
ENV PORT=8080
EXPOSE 8080

CMD ["server"]
