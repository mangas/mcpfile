FROM debian:bookworm-slim

ARG BINARY_PATH=docker-build/mcpfile-x86_64-linux

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*

COPY ${BINARY_PATH} /usr/local/bin/mcpfile
RUN chmod +x /usr/local/bin/mcpfile

ENTRYPOINT ["mcpfile"]
