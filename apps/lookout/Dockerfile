FROM rust:slim AS builder

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
