#!/bin/bash
# k8pk installation script
# Detects OS, architecture, shell, and terminal, then installs k8pk with appropriate integrations

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

error() {
    echo -e "${RED}Error:${NC} $1" >&2
    exit 1
}

info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

success() {
    echo -e "${GREEN}[OK]${NC} $1"
}

warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

# Detect OS and architecture
detect_platform() {
    local os
    local arch
    
    case "$(uname -s)" in
        Linux*)
            os="linux"
            ;;
        Darwin*)
            os="darwin"
            ;;
        MINGW*|MSYS*|CYGWIN*)
            os="windows"
            ;;
        *)
            error "Unsupported OS: $(uname -s)"
            ;;
    esac
    
    case "$(uname -m)" in
        x86_64|amd64)
            arch="x86_64"
            ;;
        arm64|aarch64)
            arch="aarch64"
            ;;
        *)
            error "Unsupported architecture: $(uname -m)"
            ;;
    esac
    
    if [ "$os" = "darwin" ] && [ "$arch" = "aarch64" ]; then
        TARGET="aarch64-apple-darwin"
    elif [ "$os" = "darwin" ] && [ "$arch" = "x86_64" ]; then
        TARGET="x86_64-apple-darwin"
    elif [ "$os" = "linux" ] && [ "$arch" = "x86_64" ]; then
        TARGET="x86_64-unknown-linux-gnu"
    elif [ "$os" = "linux" ] && [ "$arch" = "aarch64" ]; then
        TARGET="aarch64-unknown-linux-gnu"
    elif [ "$os" = "windows" ]; then
        TARGET="x86_64-pc-windows-msvc"
    else
        error "Unsupported platform: $os/$arch"
    fi
    
    info "Detected platform: $os/$arch ($TARGET)"
}

# Detect shell
detect_shell() {
    SHELL_NAME=$(basename "$SHELL" 2>/dev/null || echo "bash")
    case "$SHELL_NAME" in
        bash|zsh)
            SHELL_TYPE="bash"
            ;;
        fish)
            SHELL_TYPE="fish"
            ;;
        *)
            SHELL_TYPE="bash"
            warn "Unknown shell '$SHELL_NAME', defaulting to bash"
            ;;
    esac
    
    info "Detected shell: $SHELL_NAME"
}

# Detect terminal emulator
detect_terminal() {
    TERMINAL="unknown"
    
    # Check environment variables
    if [ -n "${TERM_PROGRAM:-}" ]; then
        TERMINAL="$TERM_PROGRAM"
    elif [ -n "${GHOSTTY:-}" ]; then
        TERMINAL="ghostty"
    elif [ -n "${WEZTERM_EXECUTABLE:-}" ]; then
        TERMINAL="wezterm"
    elif [ -n "${ITERM_SESSION_ID:-}" ]; then
        TERMINAL="iterm2"
    elif [ -n "${KITTY_WINDOW_ID:-}" ]; then
        TERMINAL="kitty"
    elif [ -n "${ALACRITTY_LOG:-}" ] || [ -n "${ALACRITTY_WINDOW_ID:-}" ]; then
        TERMINAL="alacritty"
    elif [ -n "${TMUX:-}" ]; then
        TERMINAL="tmux"
    elif [ -n "${TERM:-}" ]; then
        case "$TERM" in
            *ghostty*)
                TERMINAL="ghostty"
                ;;
            *wezterm*)
                TERMINAL="wezterm"
                ;;
            *iterm*)
                TERMINAL="iterm2"
                ;;
            *kitty*)
                TERMINAL="kitty"
                ;;
            *)
                TERMINAL="generic"
                ;;
        esac
    fi
    
    info "Detected terminal: $TERMINAL"
}

# Download and install binary
install_binary() {
    local version="${1:-latest}"
    local install_dir="${2:-/usr/local/bin}"
    
    info "Installing k8pk $version..."
    
    if [ "$version" = "latest" ]; then
        # Get latest release tag
        if command -v curl >/dev/null 2>&1; then
            VERSION=$(curl -s https://api.github.com/repos/vee-sh/k8pk/releases/latest | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')
        elif command -v wget >/dev/null 2>&1; then
            VERSION=$(wget -qO- https://api.github.com/repos/vee-sh/k8pk/releases/latest | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')
        else
            error "Need curl or wget to download k8pk"
        fi
    else
        VERSION="$version"
    fi
    
    # Remove 'v' prefix if present
    VERSION="${VERSION#v}"
    VERSION_TAG="v${VERSION}"
    
    info "Downloading k8pk $VERSION_TAG for $TARGET..."
    
    DOWNLOAD_URL="https://github.com/vee-sh/k8pk/releases/download/${VERSION_TAG}/k8pk-${VERSION_TAG}-${TARGET}.tar.gz"
    
    TEMP_DIR=$(mktemp -d)
    trap "rm -rf $TEMP_DIR" EXIT
    
    if command -v curl >/dev/null 2>&1; then
        HTTP_CODE=$(curl -L -w "%{http_code}" -o "$TEMP_DIR/k8pk.tar.gz" "$DOWNLOAD_URL" 2>/dev/null || echo "000")
        if [ "$HTTP_CODE" != "200" ]; then
            if [ "$HTTP_CODE" = "404" ]; then
                error "Release asset not found. The release may still be building. Please try again in a few minutes.\nURL: $DOWNLOAD_URL"
            else
                error "Failed to download k8pk (HTTP $HTTP_CODE). URL: $DOWNLOAD_URL"
            fi
        fi
    elif command -v wget >/dev/null 2>&1; then
        if ! wget -O "$TEMP_DIR/k8pk.tar.gz" "$DOWNLOAD_URL" 2>&1 | grep -q "200 OK"; then
            error "Failed to download k8pk. The release may still be building. Please try again in a few minutes.\nURL: $DOWNLOAD_URL"
        fi
    else
        error "Need curl or wget to download k8pk"
    fi
    
    # Verify archive before extracting
    if [ ! -s "$TEMP_DIR/k8pk.tar.gz" ]; then
        error "Downloaded file is empty. The release may still be building. Please try again in a few minutes."
    fi
    
    # Check if file is actually a tar.gz (starts with gzip magic bytes or tar signature)
    if ! file "$TEMP_DIR/k8pk.tar.gz" | grep -qE "(gzip|tar|archive)"; then
        error "Downloaded file is not a valid archive. This usually means the release is still building.\nPlease check: https://github.com/vee-sh/k8pk/releases/tag/$VERSION_TAG\nOr try again in a few minutes."
    fi
    
    tar -xzf "$TEMP_DIR/k8pk.tar.gz" -C "$TEMP_DIR" || error "Failed to extract archive. The downloaded file may be corrupted or incomplete."
    
    # Find the extracted directory
    EXTRACTED_DIR=$(find "$TEMP_DIR" -maxdepth 1 -type d -name "k8pk-*" | head -1)
    if [ -z "$EXTRACTED_DIR" ]; then
        error "Could not find extracted directory in archive"
    fi
    
    info "Found extracted directory: $EXTRACTED_DIR"
    
    # Determine install path
    if [ "$(id -u)" -eq 0 ]; then
        INSTALL_PATH="$install_dir/k8pk"
    else
        # Try user-local install first
        if [ -d "$HOME/.local/bin" ]; then
            INSTALL_PATH="$HOME/.local/bin/k8pk"
            INSTALL_DIR="$HOME/.local/bin"
        elif [ -d "$HOME/bin" ]; then
            INSTALL_PATH="$HOME/bin/k8pk"
            INSTALL_DIR="$HOME/bin"
        else
            mkdir -p "$HOME/.local/bin"
            INSTALL_PATH="$HOME/.local/bin/k8pk"
            INSTALL_DIR="$HOME/.local/bin"
        fi
    fi
    
    # Copy binary
    if [ -f "$EXTRACTED_DIR/k8pk" ]; then
        cp "$EXTRACTED_DIR/k8pk" "$INSTALL_PATH"
        chmod +x "$INSTALL_PATH"
        success "Installed k8pk to $INSTALL_PATH"
    else
        error "Binary not found in downloaded archive at $EXTRACTED_DIR/k8pk"
    fi
    
    # Extract shell scripts if available
    if [ -d "$EXTRACTED_DIR/shell" ]; then
        SHELL_SCRIPT_DIR="$EXTRACTED_DIR/shell"
        info "Found shell scripts directory: $SHELL_SCRIPT_DIR"
    else
        warn "Shell scripts directory not found in archive"
    fi
    
    # Verify installation
    if command -v k8pk >/dev/null 2>&1 || [ -x "$INSTALL_PATH" ]; then
        success "k8pk is ready to use"
        if [ -n "${INSTALL_DIR:-}" ] && [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
            warn "Add $INSTALL_DIR to your PATH:"
            echo ""
            echo "  # For bash/zsh:"
            echo "  echo 'export PATH=\"\$PATH:$INSTALL_DIR\"' >> ~/.${SHELL_NAME}rc"
            echo "  source ~/.${SHELL_NAME}rc"
            echo ""
            echo "  # Or for fish:"
            echo "  echo 'set -gx PATH \$PATH $INSTALL_DIR' >> ~/.config/fish/config.fish"
            echo ""
            echo "  # Or manually add to your shell config:"
            echo "  export PATH=\"\$PATH:$INSTALL_DIR\""
        fi
    else
        error "Installation verification failed"
    fi
}

# Install shell completions
install_completions() {
    # Always use the newly installed binary, not whatever is in PATH
    if [ ! -x "$INSTALL_PATH" ]; then
        warn "k8pk binary not found at $INSTALL_PATH, skipping completion installation"
        return
    fi
    
    local k8pk_bin="$INSTALL_PATH"
    
    info "Setting up shell completions for $SHELL_NAME..."
    
    case "$SHELL_NAME" in
        bash)
            local completion_dir
            if [ -d "/etc/bash_completion.d" ] && [ "$(id -u)" -eq 0 ]; then
                completion_dir="/etc/bash_completion.d"
            elif [ -d "$HOME/.local/share/bash-completion/completions" ]; then
                completion_dir="$HOME/.local/share/bash-completion/completions"
                mkdir -p "$completion_dir"
            else
                completion_dir="$HOME/.local/share/bash-completion/completions"
                mkdir -p "$completion_dir"
            fi
            
            if [ -w "$completion_dir" ]; then
                "$k8pk_bin" completions bash > "$completion_dir/k8pk" 2>/dev/null
                if [ $? -eq 0 ]; then
                    success "Installed bash completions to $completion_dir/k8pk"
                else
                    warn "Failed to generate bash completions"
                fi
            else
                warn "Cannot write to $completion_dir (requires sudo or user directory)"
            fi
            ;;
        zsh)
            local completion_dir="$HOME/.zsh/completions"
            mkdir -p "$completion_dir"
            
            if "$k8pk_bin" completions zsh > "$completion_dir/_k8pk" 2>/dev/null; then
                success "Installed zsh completions to $completion_dir/_k8pk"
                
                # Add to fpath if not already present
                local zshrc_file
                if [ -f "$HOME/.zshrc" ]; then
                    zshrc_file="$HOME/.zshrc"
                else
                    zshrc_file="$HOME/.zshrc"
                    touch "$zshrc_file"
                fi
                
                if ! grep -q "fpath.*zsh/completions" "$zshrc_file" 2>/dev/null; then
                    echo "" >> "$zshrc_file"
                    echo "# k8pk completions" >> "$zshrc_file"
                    echo "fpath=(\$HOME/.zsh/completions \$fpath)" >> "$zshrc_file"
                    echo "autoload -U compinit && compinit" >> "$zshrc_file"
                    info "Added fpath configuration to $zshrc_file"
                fi
            else
                warn "Failed to generate zsh completions"
            fi
            ;;
        fish)
            local completion_dir="$HOME/.config/fish/completions"
            mkdir -p "$completion_dir"
            
            if "$k8pk_bin" completions fish > "$completion_dir/k8pk.fish" 2>/dev/null; then
                success "Installed fish completions to $completion_dir/k8pk.fish"
            else
                warn "Failed to generate fish completions"
            fi
            ;;
        *)
            warn "Completions not supported for $SHELL_NAME"
            ;;
    esac
}

# Install shell integration
install_shell_integration() {
    if [ -z "${SHELL_SCRIPT_DIR:-}" ] || [ ! -d "${SHELL_SCRIPT_DIR}" ]; then
        warn "Shell scripts not available, skipping shell integration"
        info "You can manually set up shell integration later by sourcing the shell scripts"
        return
    fi
    
    info "Setting up shell integration for $SHELL_NAME..."
    
    local rc_file
    case "$SHELL_TYPE" in
        bash)
            if [ -f "$HOME/.bashrc" ]; then
                rc_file="$HOME/.bashrc"
            elif [ -f "$HOME/.bash_profile" ]; then
                rc_file="$HOME/.bash_profile"
            else
                rc_file="$HOME/.bashrc"
            fi
            SCRIPT_FILE="$SHELL_SCRIPT_DIR/k8pk.sh"
            ;;
        fish)
            mkdir -p "$HOME/.config/fish"
            rc_file="$HOME/.config/fish/config.fish"
            SCRIPT_FILE="$SHELL_SCRIPT_DIR/k8pk.fish"
            ;;
        *)
            warn "Shell integration not supported for $SHELL_NAME"
            return
            ;;
    esac
    
    if [ ! -f "$SCRIPT_FILE" ]; then
        warn "Shell script not found: $SCRIPT_FILE"
        info "Available files in shell directory:"
        ls -la "$SHELL_SCRIPT_DIR" 2>/dev/null || true
        return
    fi
    
    # Copy script to a permanent location
    K8PK_SCRIPT_DIR="$HOME/.local/share/k8pk"
    mkdir -p "$K8PK_SCRIPT_DIR"
    cp "$SCRIPT_FILE" "$K8PK_SCRIPT_DIR/"
    
    # Add source line to rc file if not already present
    SOURCE_LINE="source $K8PK_SCRIPT_DIR/$(basename $SCRIPT_FILE)"
    
    if [ -f "$rc_file" ] && grep -q "k8pk" "$rc_file" 2>/dev/null; then
        info "Shell integration already configured in $rc_file"
    else
        echo "" >> "$rc_file"
        echo "# k8pk shell integration" >> "$rc_file"
        echo "$SOURCE_LINE" >> "$rc_file"
        success "Added shell integration to $rc_file"
        info "Run 'source $rc_file' or restart your shell to use kpick and kswitch commands"
    fi
}

# Install WezTerm plugin
install_wezterm_plugin() {
    if [ "$TERMINAL" != "wezterm" ]; then
        return
    fi
    
    info "Detected WezTerm, setting up plugin..."
    
    # Find WezTerm config directory
    if [ "$(uname -s)" = "Darwin" ]; then
        WEZTERM_CONFIG_DIR="$HOME/.config/wezterm"
    else
        WEZTERM_CONFIG_DIR="$HOME/.config/wezterm"
    fi
    
    mkdir -p "$WEZTERM_CONFIG_DIR"
    
    # Check if config file exists
    if [ -f "$WEZTERM_CONFIG_DIR/wezterm.lua" ]; then
        CONFIG_FILE="$WEZTERM_CONFIG_DIR/wezterm.lua"
    else
        CONFIG_FILE="$WEZTERM_CONFIG_DIR/wezterm.lua"
        # Create basic config file
        cat > "$CONFIG_FILE" <<'EOF'
local wezterm = require 'wezterm'
local config = wezterm.config_builder and wezterm.config_builder() or {}

-- k8pk plugin will be added here

return config
EOF
    fi
    
    # Check if plugin is already configured
    if grep -q "k8pk\|k8s_power" "$CONFIG_FILE" 2>/dev/null; then
        info "WezTerm plugin already configured"
    else
        # Add plugin configuration
        PLUGIN_CONFIG=$(cat <<'PLUGIN'
local k8s_power = wezterm.plugin.require('https://github.com/vee-sh/k8pk')
k8s_power.apply_to_config(config)
PLUGIN
)
        
        # Insert before return statement
        if grep -q "^return config" "$CONFIG_FILE"; then
            sed -i.bak "/^return config/i\\
$PLUGIN_CONFIG
" "$CONFIG_FILE"
        else
            echo "" >> "$CONFIG_FILE"
            echo "$PLUGIN_CONFIG" >> "$CONFIG_FILE"
        fi
        
        success "Added WezTerm plugin configuration"
        info "Restart WezTerm to activate the plugin"
    fi
}

# Main installation
main() {
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "  k8pk Installation Script"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    
    VERSION="${1:-latest}"
    
    detect_platform
    detect_shell
    detect_terminal
    echo ""
    
    install_binary "$VERSION"
    echo ""
    
    install_completions
    echo ""
    
    install_shell_integration
    echo ""
    
    install_wezterm_plugin
    echo ""
    
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    success "Installation complete!"
    echo ""
    echo "Quick start:"
    echo "  1. Restart your shell or run: source ~/.${SHELL_NAME}rc (or ~/.config/fish/config.fish for fish)"
    echo "  2. Use 'kpick' for interactive context/namespace selection"
    echo "  3. Use 'kswitch <context> [namespace]' for quick switching"
    echo ""
    if [ "$TERMINAL" = "wezterm" ]; then
        echo "  WezTerm: Press CTRL+SHIFT+K to open the picker"
    fi
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
}

# Run main function
main "$@"

