#!/bin/sh
# Installer script for aps (Agentic Prompt Sync)
# Usage: curl -fsSL https://raw.githubusercontent.com/westonplatter/aps/main/install.sh | sh

set -e

REPO="westonplatter/aps"
BINARY_NAME="aps"
INSTALL_DIR="${APS_INSTALL_DIR:-$HOME/.local/bin}"

# Colors (if terminal supports them)
if [ -t 1 ]; then
    RED='\033[0;31m'
    GREEN='\033[0;32m'
    YELLOW='\033[0;33m'
    NC='\033[0m' # No Color
else
    RED=''
    GREEN=''
    YELLOW=''
    NC=''
fi

info() {
    printf "${GREEN}info${NC}: %s\n" "$1"
}

warn() {
    printf "${YELLOW}warn${NC}: %s\n" "$1"
}

error() {
    printf "${RED}error${NC}: %s\n" "$1" >&2
    exit 1
}

# Detect OS
detect_os() {
    case "$(uname -s)" in
        Linux*)  echo "linux" ;;
        Darwin*) echo "macos" ;;
        MINGW*|MSYS*|CYGWIN*) echo "windows" ;;
        *)       error "Unsupported operating system: $(uname -s)" ;;
    esac
}

# Detect architecture
detect_arch() {
    case "$(uname -m)" in
        x86_64|amd64)  echo "x64" ;;
        aarch64|arm64) echo "arm64" ;;
        *)             error "Unsupported architecture: $(uname -m)" ;;
    esac
}

# Get the download URL for the latest release
get_download_url() {
    local os="$1"
    local arch="$2"
    local artifact_name="${BINARY_NAME}-${os}-${arch}"

    # For Linux x64, prefer musl for better portability
    if [ "$os" = "linux" ] && [ "$arch" = "x64" ]; then
        artifact_name="${BINARY_NAME}-linux-x64-musl"
    fi

    local ext="tar.gz"
    if [ "$os" = "windows" ]; then
        ext="zip"
    fi

    # Get latest release tag
    local latest_tag
    latest_tag=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/')

    if [ -z "$latest_tag" ]; then
        error "Could not determine latest release. Check https://github.com/${REPO}/releases"
    fi

    echo "https://github.com/${REPO}/releases/download/${latest_tag}/${artifact_name}.${ext}"
}

main() {
    info "Installing aps (Agentic Prompt Sync)..."

    local os=$(detect_os)
    local arch=$(detect_arch)

    info "Detected: ${os}-${arch}"

    # Check for required tools
    if ! command -v curl >/dev/null 2>&1; then
        error "curl is required but not installed"
    fi

    # Get download URL
    local url
    url=$(get_download_url "$os" "$arch")
    info "Downloading from: ${url}"

    # Create temp directory
    local tmpdir
    tmpdir=$(mktemp -d)
    trap 'rm -rf "$tmpdir"' EXIT

    # Download and extract
    local archive="${tmpdir}/aps-archive"
    if ! curl -fsSL "$url" -o "$archive"; then
        error "Failed to download aps. Make sure a release exists at https://github.com/${REPO}/releases"
    fi

    if [ "$os" = "windows" ]; then
        unzip -q "$archive" -d "$tmpdir"
    else
        tar -xzf "$archive" -C "$tmpdir"
    fi

    # Create install directory if needed
    mkdir -p "$INSTALL_DIR"

    # Install binary
    local binary_path="${tmpdir}/${BINARY_NAME}"
    if [ "$os" = "windows" ]; then
        binary_path="${binary_path}.exe"
    fi

    if [ ! -f "$binary_path" ]; then
        error "Binary not found in archive"
    fi

    mv "$binary_path" "${INSTALL_DIR}/${BINARY_NAME}"
    chmod +x "${INSTALL_DIR}/${BINARY_NAME}"

    info "Installed to: ${INSTALL_DIR}/${BINARY_NAME}"

    # Check if install dir is in PATH
    case ":$PATH:" in
        *":${INSTALL_DIR}:"*) ;;
        *)
            warn "${INSTALL_DIR} is not in your PATH"
            echo ""
            echo "Add it to your shell configuration:"
            echo "  export PATH=\"\$PATH:${INSTALL_DIR}\""
            echo ""
            ;;
    esac

    info "Installation complete! Run 'aps --help' to get started."
}

main "$@"
