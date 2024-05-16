FROM rust:1.78.0 as builder

WORKDIR /usr/src/jivachat

COPY . .

RUN cargo build --release

FROM debian:bookworm-slim

COPY --from=builder /usr/src/jivachat/target/release ./release

RUN apt-get update && apt-get install -y libc6 ca-certificates

EXPOSE 3030

CMD ["./release/jivachat"]
