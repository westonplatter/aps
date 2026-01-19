# Rust Build Distribution

This document describes strategies for building and distributing Rust binaries across multiple platforms (Windows, Linux, macOS).

## Target Platforms

| Platform    | Target Triple               | Notes                            |
| ----------- | --------------------------- | -------------------------------- |
| Linux x64   | `x86_64-unknown-linux-gnu`  | Most common Linux target         |
| Linux ARM   | `aarch64-unknown-linux-gnu` | ARM64 Linux (AWS Graviton, etc.) |
| macOS Intel | `x86_64-apple-darwin`       | Intel Macs                       |
| macOS ARM   | `aarch64-apple-darwin`      | Apple Silicon (M1/M2/M3)         |
| Windows x64 | `x86_64-pc-windows-msvc`    | Windows with MSVC toolchain      |

## Build Strategies

### 1. GitHub Actions CI/CD (Recommended)

Build natively on each platform using GitHub-hosted runners. This is the most reliable approach since each binary is built on its native OS.

```yaml
# .github/workflows/release.yml
name: Release

on:
  push:
    tags:
      - "v*"

jobs:
  build:
    strategy:
      matrix:
        include:
          - os: ubuntu-latest
            target: x86_64-unknown-linux-gnu
            artifact: my-app-linux-x64
          - os: macos-latest
            target: aarch64-apple-darwin
            artifact: my-app-macos-arm64
          - os: macos-13
            target: x86_64-apple-darwin
            artifact: my-app-macos-x64
          - os: windows-latest
            target: x86_64-pc-windows-msvc
            artifact: my-app-windows-x64
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}
      - run: cargo build --release --target ${{ matrix.target }}
      - uses: actions/upload-artifact@v4
        with:
          name: ${{ matrix.artifact }}
          path: target/${{ matrix.target }}/release/my-app*
```

### 2. cargo-dist (Automated Setup)

[cargo-dist](https://opensource.axo.dev/cargo-dist/) generates release workflows and installers automatically.

```bash
# Install cargo-dist
cargo install cargo-dist

# Initialize in your project
cargo dist init

# Generate CI configuration
cargo dist generate
```

Features:

- GitHub Releases with pre-built binaries
- Shell installer scripts (curl | sh)
- PowerShell installer scripts
- Homebrew formula generation
- Checksums and optional signing

### 3. Cross-Compilation

Build for multiple targets from a single machine.

**Using `cross`:**

```bash
cargo install cross
cross build --release --target x86_64-unknown-linux-gnu
cross build --release --target aarch64-unknown-linux-gnu
```

**Using `cargo-zigbuild`:**

```bash
cargo install cargo-zigbuild
cargo zigbuild --release --target x86_64-unknown-linux-gnu
```

## Distribution Channels

### GitHub Releases

Attach binaries to GitHub releases. Users download directly.

### crates.io

Publish source to crates.io for `cargo install`:

```bash
cargo publish
```

### Homebrew

Create a tap repository or submit to homebrew-core:

```ruby
# Formula/my-app.rb
class MyApp < Formula
  desc "Description of my app"
  homepage "https://github.com/user/my-app"
  url "https://github.com/user/my-app/releases/download/v1.0.0/my-app-macos-arm64.tar.gz"
  sha256 "..."
end
```

### cargo-binstall

If binaries are published to GitHub releases with standard naming, users can install without compiling:

```bash
cargo binstall my-app
```

## Considerations

### Static vs Dynamic Linking (Linux)

- `x86_64-unknown-linux-gnu` - dynamically links glibc
- `x86_64-unknown-linux-musl` - statically linked, more portable

For maximum Linux compatibility, consider musl targets:

```bash
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl
```

### Universal Binaries (macOS)

Create a universal binary that runs on both Intel and Apple Silicon:

```bash
cargo build --release --target x86_64-apple-darwin
cargo build --release --target aarch64-apple-darwin
lipo -create -output my-app-universal \
  target/x86_64-apple-darwin/release/my-app \
  target/aarch64-apple-darwin/release/my-app
```

### Code Signing

- **macOS**: Notarization required for distribution outside App Store
- **Windows**: Code signing certificates for trusted downloads

## Next Steps

1. Choose a distribution strategy based on project needs
2. Set up GitHub Actions workflow for automated builds
3. Configure release automation (manual tags or semantic-release)
4. Add installation instructions to README
