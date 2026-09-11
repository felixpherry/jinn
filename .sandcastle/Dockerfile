FROM node:24-bookworm

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential ca-certificates curl git gh jq pkg-config libssl-dev \
    libsqlite3-dev clang lld cmake python3 unzip \
    && rm -rf /var/lib/apt/lists/*

ARG AGENT_UID=1000
ARG AGENT_GID=1000
RUN groupmod -o -g ${AGENT_GID} node \
    && usermod -o -u ${AGENT_UID} -g ${AGENT_GID} -d /home/agent -m -l agent node

ARG PI_VERSION=0.85.1
ARG CLAUDE_VERSION=2.1.268
RUN npm install -g --ignore-scripts @earendil-works/pi-coding-agent@${PI_VERSION} \
    @anthropic-ai/claude-code@${CLAUDE_VERSION} \
    && node /usr/local/lib/node_modules/@anthropic-ai/claude-code/install.cjs

USER ${AGENT_UID}:${AGENT_GID}
ENV HOME=/home/agent
ENV PATH=/home/agent/.cargo/bin:$PATH
ARG RUST_VERSION=1.95.0
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --default-toolchain ${RUST_VERSION} \
    && rustup component add rustfmt clippy \
    && rustup target add wasm32-wasip2 \
    && cargo install just --locked --version 1.58.0

WORKDIR /home/agent
ENTRYPOINT ["sleep", "infinity"]
