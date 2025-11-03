FROM rust:1.70 as builder
WORKDIR /usr/src/app
COPY . .
RUN cargo build --release

FROM debian:bullseye-slim
RUN apt-get update && apt-get install -y \
    python3 \
    nodejs \
    golang \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/src/app/target/release/cylo /usr/local/bin/cylo
CMD ["cylo"]
