#!/bin/sh
# Install script for database-mcp
# Usage: curl -fsSL https://database.haymon.ai/install.sh | bash
#    or: wget -qO- https://database.haymon.ai/install.sh | bash
#    or: sh install.sh
#
# Environment variables:
#   INSTALL_DIR - Override the install directory (e.g., INSTALL_DIR=/opt/bin)

set -eu

main() {
    # T002: TTY detection and color setup
    if [ -t 1 ]; then
        BOLD='\033[1m'
        GREEN='\033[0;32m'
        YELLOW='\033[0;33m'
        RED='\033[0;31m'
        RESET='\033[0m'
    else
        BOLD=''
        GREEN=''
        YELLOW=''
        RED=''
        RESET=''
    fi

    # T003: Utility functions
    info() {
        printf "${BOLD}%s${RESET}\n" "$1"
    }

    warn() {
        printf "${YELLOW}warning${RESET}: %s\n" "$1" >&2
    }

    error() {
        printf "${RED}error${RESET}: %s\n" "$1" >&2
        exit 1
    }

    success() {
        printf "${GREEN}%s${RESET}\n" "$1"
    }

    # T004: Download helper
    download() {
        _url="$1"
        _output="$2"
        if command -v curl > /dev/null 2>&1; then
            curl -fsSL "$_url" -o "$_output"
        elif command -v wget > /dev/null 2>&1; then
            wget -qO "$_output" "$_url"
        else
            error "either 'curl' or 'wget' is required to download files"
        fi
    }

    # T005: Platform detection
    detect_platform() {
        _os=$(uname -s)
        _arch=$(uname -m)

        case "$_os" in
            Linux)  PLATFORM_OS="linux" ;;
            Darwin) PLATFORM_OS="darwin" ;;
            *)      error "unsupported operating system: $_os (supported: Linux, macOS)" ;;
        esac

        case "$_arch" in
            x86_64|amd64)
                PLATFORM_ARCH="x86_64"
                # Rosetta detection on macOS
                if [ "$PLATFORM_OS" = "darwin" ]; then
                    _translated=$(sysctl -n sysctl.proc_translated 2>/dev/null || echo "0")
                    if [ "$_translated" = "1" ]; then
                        PLATFORM_ARCH="aarch64"
                    fi
                fi
                ;;
            aarch64|arm64)
                PLATFORM_ARCH="aarch64"
                ;;
            *)
                error "unsupported architecture: $_arch (supported: x86_64, aarch64/arm64)"
                ;;
        esac
    }

    # T006: Target triple mapping
    get_target() {
        case "${PLATFORM_OS}-${PLATFORM_ARCH}" in
            linux-x86_64)   TARGET="x86_64-unknown-linux-gnu" ;;
            linux-aarch64)  TARGET="aarch64-unknown-linux-gnu" ;;
            darwin-x86_64)  TARGET="x86_64-apple-darwin" ;;
            darwin-aarch64) TARGET="aarch64-apple-darwin" ;;
            *)              error "no prebuilt binary for ${PLATFORM_OS}-${PLATFORM_ARCH}" ;;
        esac
    }

    # T007: Install directory resolution
    resolve_install_dir() {
        UPGRADE=false
        OLD_VERSION=""

        # Priority 1: INSTALL_DIR env var
        if [ -n "${INSTALL_DIR:-}" ]; then
            BIN_DIR="$INSTALL_DIR"
            if [ ! -d "$BIN_DIR" ]; then
                mkdir -p "$BIN_DIR" 2>/dev/null || error "cannot create directory: $BIN_DIR"
            fi
            return
        fi

        # Priority 2: Existing binary detection (upgrade in-place)
        _existing=$(command -v database-mcp 2>/dev/null || true)
        if [ -n "$_existing" ]; then
            # Resolve symlinks to find the actual binary location
            _existing=$(readlink -f "$_existing" 2>/dev/null || echo "$_existing")
            BIN_DIR=$(dirname "$_existing")
            UPGRADE=true
            OLD_VERSION=$("$_existing" --version 2>/dev/null || echo "unknown")
            return
        fi

        # Priority 3: Default logic
        # 3a: /usr/local/bin if writable
        if [ -w "/usr/local/bin" ]; then
            BIN_DIR="/usr/local/bin"
            return
        fi

        # 3b: /usr/local/bin with sudo (if interactive)
        if [ -t 0 ] && command -v sudo > /dev/null 2>&1; then
            if sudo -n true 2>/dev/null; then
                # sudo available without password
                BIN_DIR="/usr/local/bin"
                USE_SUDO=true
                return
            fi
            # Try prompting for password
            info "Administrator access is needed to install to /usr/local/bin"
            if sudo true 2>/dev/null; then
                BIN_DIR="/usr/local/bin"
                USE_SUDO=true
                return
            fi
        fi

        # 3c: Fallback to ~/.local/bin
        BIN_DIR="$HOME/.local/bin"
        if [ ! -d "$BIN_DIR" ]; then
            mkdir -p "$BIN_DIR"
        fi
    }

    # T013/T014: Upgrade messaging integrated into resolve_install_dir above

    # T016: PATH check and instructions
    check_path() {
        _dir="$1"
        case ":${PATH}:" in
            *":${_dir}:"*) return ;; # already in PATH
        esac

        echo ""
        warn "$_dir is not in your PATH"
        echo ""

        _shell_name=$(basename "${SHELL:-/bin/sh}")
        case "$_shell_name" in
            bash)
                echo "  Add it to your PATH by running:"
                echo ""
                echo "    echo 'export PATH=\"$_dir:\$PATH\"' >> ~/.bashrc"
                echo "    source ~/.bashrc"
                ;;
            zsh)
                echo "  Add it to your PATH by running:"
                echo ""
                echo "    echo 'export PATH=\"$_dir:\$PATH\"' >> ~/.zshrc"
                echo "    source ~/.zshrc"
                ;;
            fish)
                echo "  Add it to your PATH by running:"
                echo ""
                echo "    fish_add_path $_dir"
                ;;
            *)
                echo "  Add it to your PATH by adding this to your shell profile:"
                echo ""
                echo "    export PATH=\"$_dir:\$PATH\""
                ;;
        esac
        echo ""
    }

    # --- Main flow (T008, T009-T012) ---

    BINARY_NAME="database-mcp"
    REPO="haymon-ai/database"
    BASE_URL="https://github.com/${REPO}/releases/latest/download"
    USE_SUDO=false

    info "Installing database-mcp..."
    echo ""

    # T005-T006: Detect platform and target
    detect_platform
    get_target
    info "Detected platform: ${PLATFORM_OS}/${PLATFORM_ARCH} (${TARGET})"

    # T007: Resolve install directory
    resolve_install_dir

    if [ "$UPGRADE" = true ]; then
        info "Upgrading database-mcp at ${BIN_DIR} (current: ${OLD_VERSION})"
    else
        info "Install directory: ${BIN_DIR}"
    fi

    # T008: Temp directory with cleanup trap
    _tmpdir=$(mktemp -d 2>/dev/null || mktemp -d -t database-mcp)
    trap 'rm -rf "$_tmpdir"' EXIT

    # T009: Download
    _asset="${BINARY_NAME}-${TARGET}.tar.gz"
    _url="${BASE_URL}/${_asset}"
    _archive="${_tmpdir}/${_asset}"

    info "Downloading ${_asset}..."
    download "$_url" "$_archive" || error "download failed — check your network connection or try again later"

    # T010: Extract and install
    tar xzf "$_archive" -C "$_tmpdir" || error "failed to extract archive"

    _binary="${_tmpdir}/${BINARY_NAME}"
    if [ ! -f "$_binary" ]; then
        error "binary '${BINARY_NAME}' not found in archive"
    fi

    chmod +x "$_binary"

    _dest="${BIN_DIR}/${BINARY_NAME}"
    if [ "$USE_SUDO" = true ]; then
        sudo install -m 755 "$_binary" "$_dest" || error "failed to install binary to ${BIN_DIR} (sudo)"
    else
        install -m 755 "$_binary" "$_dest" 2>/dev/null || cp "$_binary" "$_dest" || error "failed to install binary to ${BIN_DIR}"
    fi

    # T011: Post-install verification
    _installed_version=$("$_dest" --version 2>/dev/null || true)
    if [ -z "$_installed_version" ]; then
        error "installation verification failed — ${_dest} --version did not produce output"
    fi

    echo ""
    if [ "$UPGRADE" = true ]; then
        success "Successfully upgraded database-mcp!"
        echo "  ${OLD_VERSION} → ${_installed_version}"
    else
        success "Successfully installed database-mcp!"
        echo "  Version: ${_installed_version}"
    fi
    echo "  Location: ${_dest}"

    # T016: Check PATH
    check_path "$BIN_DIR"
}

main "$@"
