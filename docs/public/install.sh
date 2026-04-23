#!/bin/sh
# Install script for dbmcp
# Usage: curl -fsSL https://dbmcp.haymon.ai/install.sh | bash
#    or: wget -qO- https://dbmcp.haymon.ai/install.sh | bash
#    or: sh install.sh
#
# Environment variables:
#   INSTALL_DIR   - Override the install directory (e.g., INSTALL_DIR=/opt/bin)
#   FORCE_INSTALL - When set to a truthy value (1/true/yes/on/y, case-insensitive)
#                   reinstall even if the installed version already matches the
#                   latest published release. Without this, re-running the
#                   script on an up-to-date install is a no-op (no download,
#                   no file writes).
#
# The script probes https://github.com/haymon-ai/dbmcp/releases/latest
# (a HEAD request that follows redirects) to learn the latest version. The
# no-op path performs zero downloads and zero writes to the install directory.

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

    # Return 0 if $1 is a truthy env-var value (1/true/yes/on/y, case-insensitive).
    is_truthy() {
        case "$1" in
            1|[Tt][Rr][Uu][Ee]|[Yy][Ee][Ss]|[Oo][Nn]|[Yy]) return 0 ;;
            *) return 1 ;;
        esac
    }

    # Normalise a version string for comparison: trim whitespace, strip
    # binary-name prefix, strip a leading `v`. Uses `-e ... -e ...` instead of
    # `;` for BSD sed compatibility (macOS sed is fine with `;` but `-e` is
    # universally portable).
    normalize_version() {
        _v=$(printf '%s' "$1" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')
        _v=${_v#dbmcp }
        _v=${_v#v}
        printf '%s' "$_v"
    }

    # Portable "is version $1 strictly greater than version $2?" comparator.
    # Uses pure POSIX shell: no `sort -V` (GNU-only, missing on macOS BSD).
    # Both inputs must already be normalised (no `v` prefix, no whitespace).
    # Pre-release / build suffixes are dropped (`0.7.0-dev` -> `0.7.0`) to
    # match what `Test-InstalledNewerThanLatest` does in install.ps1.
    version_gt() {
        _va=${1%%-*}
        _vb=${2%%-*}
        # Split on `.` via IFS. Function-local IFS via a subshell would be
        # cleaner but costs a fork per call; save/restore instead.
        _ifs_save=$IFS
        IFS=.
        # shellcheck disable=SC2086  # intentional word splitting
        set -- $_va
        _a1=${1:-0}; _a2=${2:-0}; _a3=${3:-0}
        # shellcheck disable=SC2086
        set -- $_vb
        _b1=${1:-0}; _b2=${2:-0}; _b3=${3:-0}
        IFS=$_ifs_save
        # Guard against non-numeric components (e.g. `rc1`) by rejecting the
        # comparison and returning "not newer" so the caller falls through to
        # the normal upgrade path without warning.
        case "${_a1}${_a2}${_a3}${_b1}${_b2}${_b3}" in
            *[!0-9]*) return 1 ;;
            *) ;;
        esac
        [ "$_a1" -gt "$_b1" ] && return 0
        [ "$_a1" -lt "$_b1" ] && return 1
        [ "$_a2" -gt "$_b2" ] && return 0
        [ "$_a2" -lt "$_b2" ] && return 1
        [ "$_a3" -gt "$_b3" ] && return 0
        return 1
    }

    # Resolve the latest release tag (e.g. `v0.6.2`) by following the
    # `releases/latest` redirect. Prints the tag on stdout and returns 0 on
    # success; returns 1 (with no output) if the lookup fails for any reason.
    # A short connect timeout ensures slow/broken networks fail fast rather
    # than hanging at curl's default 2-minute connect timeout — the no-op
    # path is supposed to be quick or not at all.
    resolve_latest_version() {
        _latest_url="https://github.com/${REPO}/releases/latest"
        _final=""
        if command -v curl > /dev/null 2>&1; then
            _final=$(curl -fsSLI --connect-timeout 10 --max-time 20 \
                -o /dev/null -w '%{url_effective}' "$_latest_url" 2>/dev/null) \
                || return 1
        elif command -v wget > /dev/null 2>&1; then
            _final=$(wget --server-response --max-redirect=5 --spider \
                --timeout=20 --tries=1 "$_latest_url" 2>&1 \
                | awk '/^[[:space:]]*Location:/ {loc=$2} END {print loc}') \
                || return 1
        else
            return 1
        fi
        _tag=${_final##*/}
        [ -n "$_tag" ] || return 1
        case "$_tag" in
            v[0-9]*|[0-9]*) printf '%s' "$_tag"; return 0 ;;
            *) return 1 ;;
        esac
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
            # If a binary already exists at the override location, treat as an
            # upgrade so the no-op check applies to the binary that will
            # actually be replaced (spec Edge Cases).
            if [ -x "$BIN_DIR/$BINARY_NAME" ]; then
                UPGRADE=true
                OLD_VERSION=$("$BIN_DIR/$BINARY_NAME" --version 2>/dev/null || echo "unknown")
            fi
            return
        fi

        # Priority 2: Existing binary detection (upgrade in-place)
        _existing=$(command -v dbmcp 2>/dev/null || true)
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

    BINARY_NAME="dbmcp"
    REPO="haymon-ai/dbmcp"
    BASE_URL="https://github.com/${REPO}/releases/latest/download"
    USE_SUDO=false
    FORCE_REINSTALL=false

    info "Installing dbmcp..."
    echo ""

    # T005-T006: Detect platform and target
    detect_platform
    get_target
    info "Detected platform: ${PLATFORM_OS}/${PLATFORM_ARCH} (${TARGET})"

    # T007: Resolve install directory
    resolve_install_dir

    if [ "$UPGRADE" = true ]; then
        info "Found existing dbmcp at ${BIN_DIR} (${OLD_VERSION})"

        # No-op / force / newer-than-latest check. Only runs when an existing
        # binary was detected. Skipped entirely if the version lookup fails,
        # so a network issue never silently claims "already up to date".
        _latest_version=$(resolve_latest_version 2>/dev/null || printf '')
        if [ -n "$_latest_version" ] && [ "$OLD_VERSION" != "unknown" ]; then
            _installed_norm=$(normalize_version "$OLD_VERSION")
            _latest_norm=$(normalize_version "$_latest_version")

            if [ "$_installed_norm" = "$_latest_norm" ]; then
                if is_truthy "${FORCE_INSTALL:-}"; then
                    FORCE_REINSTALL=true
                    info "FORCE_INSTALL is set - reinstalling ${_latest_version}"
                else
                    success "Already on latest version (${_latest_version}). Nothing to do."
                    return 0
                fi
            else
                # Versions differ. If the installed side is strictly newer
                # than the latest, warn loudly before proceeding with a
                # downgrade (see FR-011a). On non-numeric versions (e.g.
                # `rc1`) `version_gt` returns "not newer" and we silently
                # fall through to the normal upgrade path.
                if version_gt "$_installed_norm" "$_latest_norm"; then
                    warn "installed ${OLD_VERSION} is newer than the latest release ${_latest_version}; proceeding will downgrade it."
                fi
                info "Upgrading to ${_latest_version}"
            fi
        else
            info "Upgrading dbmcp at ${BIN_DIR} (current: ${OLD_VERSION})"
        fi
    else
        info "Install directory: ${BIN_DIR}"
    fi

    # T008: Temp directory with cleanup trap
    _tmpdir=$(mktemp -d 2>/dev/null || mktemp -d -t dbmcp)
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
    if [ "$FORCE_REINSTALL" = true ]; then
        success "Successfully reinstalled dbmcp!"
        echo "  Version: ${_installed_version}"
    elif [ "$UPGRADE" = true ]; then
        success "Successfully upgraded dbmcp!"
        echo "  ${OLD_VERSION} → ${_installed_version}"
    else
        success "Successfully installed dbmcp!"
        echo "  Version: ${_installed_version}"
    fi
    echo "  Location: ${_dest}"

    # T016: Check PATH
    check_path "$BIN_DIR"
}

main "$@"
