image_repo := "mangas/mcpfile"
tag := `echo ${TAG:-$(git rev-parse --abbrev-ref HEAD | tr '/' '-')}`

default: help

help:
    @just -l

# Run clippy with warnings as errors
lint:
    cargo clippy --all-targets --all-features -- -D warnings

# Run clippy with auto-fix
lint-fix:
    cargo clippy --all-targets --all-features --fix -- -D warnings

# Check formatting
fmt:
    cargo fmt -- --check

# Auto-format code
fmt-fix:
    cargo fmt

# Build release binary
build:
    cargo build --release

# Run unit tests
test:
    cargo test -- --nocapture

# Run all tests including integration (requires Docker)
test-all:
    cargo test -- --nocapture --include-ignored

# Build Docker image
docker-build newTag=tag:
    docker build -t {{ image_repo }}:{{ newTag }} .

# Push Docker image
docker-push newTag=tag:
    docker push {{ image_repo }}:{{ newTag }}

# Build and push Docker image
docker-build-push newTag=tag: (docker-build newTag) (docker-push newTag)

# Install locally
install:
    cargo install --path .

# Install fish completions
install-completions:
    mcpfile completions fish > ~/.config/fish/completions/mcpfile.fish

# Install Claude Code skill
install-skill:
    mcpfile install-skill
