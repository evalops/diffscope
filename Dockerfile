# Build stage
FROM rust:alpine AS builder

RUN apk add --no-cache musl-dev

WORKDIR /app

# Cache dependencies
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs && \
    cargo build --release && \
    rm -rf src

# Build actual binary
COPY src ./src
RUN touch src/main.rs && cargo build --release

# Runtime stage
FROM alpine:3.19

RUN apk add --no-cache ca-certificates git

COPY --from=builder /app/target/release/diffscope /usr/local/bin/diffscope

ENTRYPOINT ["diffscope"]
CMD ["--help"]
