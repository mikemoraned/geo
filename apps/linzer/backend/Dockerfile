FROM rust:slim-buster AS builder

WORKDIR /prod

COPY . .
RUN cargo build --release

FROM fedora:34 AS runner
COPY --from=builder /prod/target/release/api /bin
RUN ls -R .

CMD ./bin/api