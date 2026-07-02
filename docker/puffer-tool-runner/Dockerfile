FROM rust:1-bookworm AS builder

WORKDIR /src

RUN apt-get update \
    && apt-get install -y --no-install-recommends protobuf-compiler pkg-config \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY benchmark/genskill/ladybird ./benchmark/genskill/ladybird
COPY vendor/grammers-session ./vendor/grammers-session
COPY resources ./resources

RUN cargo build --release -p puffer-tool-runner

FROM debian:bookworm-slim

ENV DEBIAN_FRONTEND=noninteractive
ENV HOME=/root
ENV PUFFER_HOME=/root
ENV PUFFER_BUILTIN_RESOURCES_DIR=/opt/puffer/resources
ENV MANAGED_AGENT_USER_PATH=/user
ENV MANAGED_AGENT_PUFFER_RUNNER_BIND=0.0.0.0:50051

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
       bash ca-certificates curl file git python3 ripgrep \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /src/target/release/puffer-tool-runner /usr/local/bin/puffer-tool-runner
COPY resources /opt/puffer/resources
COPY docker/puffer-tool-runner/entrypoint.sh /usr/local/bin/puffer-tool-runner-entrypoint.sh

RUN chmod +x /usr/local/bin/puffer-tool-runner /usr/local/bin/puffer-tool-runner-entrypoint.sh \
    && mkdir -p /user /workspace /root/.puffer \
    && puffer-tool-runner --help >/dev/null

WORKDIR /user
EXPOSE 50051

ENTRYPOINT ["/usr/local/bin/puffer-tool-runner-entrypoint.sh"]
