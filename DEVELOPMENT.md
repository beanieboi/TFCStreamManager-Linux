# Development Guide

## Requirements

- Rust 1.85+
- GTK4 development libraries

## Building from Source

```bash
# Clone the repository
git clone https://github.com/beanieboi/TFCStreamManager-linux.git
cd TFCStreamManager-linux

# Build release version
cargo build --release

# The binary will be at target/release/tfc_stream_manager
```

### Dependencies (Debian/Ubuntu)

```bash
sudo apt install libgtk-4-dev libssl-dev pkg-config libdbus-1-dev
```

### Dependencies (Fedora)

```bash
sudo dnf install gtk4-devel openssl-devel dbus-devel
```

### Dependencies (Arch)

```bash
sudo pacman -S gtk4 openssl dbus
```

## Technology Stack

- **GUI:** GTK4
- **Async Runtime:** Tokio
- **Web Server:** Axum
- **HTTP Client:** Reqwest
- **Service Discovery:** mdns-sd
- **Secret Storage:** keyring (Secret Service)

## Running Tests

```bash
cargo test
```

## Code Style

```bash
cargo fmt
cargo clippy
```
