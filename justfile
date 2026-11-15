default: run

# Run the app (debug build)
run *args:
    cargo run -- {{args}}

# Run the app (release build)
release *args:
    cargo run --release -- {{args}}

# Run with debug logging
debug *args:
    RUST_LOG=tfc_stream_manager=debug cargo run -- {{args}}

build:
    cargo build --release

test:
    cargo test

fmt:
    cargo fmt --all

lint:
    cargo fmt --all -- --check
    cargo clippy --all-targets -- -D warnings

# Run the same checks as CI
ci: lint test
