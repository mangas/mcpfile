FROM scratch

ARG BINARY_PATH=docker-build/mcpfile-x86_64-linux

COPY ${BINARY_PATH} /mcpfile

ENTRYPOINT ["/mcpfile"]
