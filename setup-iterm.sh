#!/bin/bash
# Setup script for k8pk in iTerm2

set -e

echo "=== Setting up k8pk for iTerm2 ==="
echo

# 1. Build k8pk if not already built
if [ ! -f "rust/k8pk/target/release/k8pk" ]; then
    echo "1. Building k8pk..."
    cd rust/k8pk
    cargo build --release
    cd ../..
    echo "   ✓ Built successfully"
else
    echo "1. k8pk already built"
fi

# 2. Install to /usr/local/bin (or show instructions)
INSTALL_PATH="/usr/local/bin/k8pk"
if [ -f "rust/k8pk/target/release/k8pk" ]; then
    echo "2. Installing k8pk to $INSTALL_PATH..."
    sudo install -m 0755 rust/k8pk/target/release/k8pk "$INSTALL_PATH"
    echo "   ✓ Installed to $INSTALL_PATH"
    
    # Verify installation
    if command -v k8pk >/dev/null 2>&1; then
        echo "   ✓ k8pk is now in PATH: $(which k8pk)"
    fi
else
    echo "   ✗ k8pk binary not found"
    exit 1
fi

# 3. Detect shell and provide setup instructions
echo
echo "3. Shell integration setup:"
SHELL_TYPE=$(basename "$SHELL")

if [ "$SHELL_TYPE" = "zsh" ]; then
    RC_FILE="$HOME/.zshrc"
elif [ "$SHELL_TYPE" = "bash" ]; then
    RC_FILE="$HOME/.bashrc"
    if [ ! -f "$RC_FILE" ]; then
        RC_FILE="$HOME/.bash_profile"
    fi
else
    RC_FILE=""
fi

if [ -n "$RC_FILE" ]; then
    SCRIPT_PATH="$(pwd)/shell/k8pk.sh"
    if [ -f "$SCRIPT_PATH" ]; then
        echo "   Detected shell: $SHELL_TYPE"
        echo "   RC file: $RC_FILE"
        echo
        echo "   Add this line to your $RC_FILE:"
        echo "   source $SCRIPT_PATH"
        echo
        read -p "   Add it automatically? (y/n) " -n 1 -r
        echo
        if [[ $REPLY =~ ^[Yy]$ ]]; then
            if ! grep -q "source $SCRIPT_PATH" "$RC_FILE" 2>/dev/null; then
                echo "" >> "$RC_FILE"
                echo "# k8pk shell integration" >> "$RC_FILE"
                echo "source $SCRIPT_PATH" >> "$RC_FILE"
                echo "   ✓ Added to $RC_FILE"
            else
                echo "   ℹ Already exists in $RC_FILE"
            fi
        fi
    fi
else
    echo "   ⚠ Unsupported shell: $SHELL_TYPE"
    echo "   Please manually source shell/k8pk.sh in your shell config"
fi

echo
echo "4. Testing installation..."
if command -v k8pk >/dev/null 2>&1; then
    echo "   ✓ k8pk command available"
    k8pk --help | head -3
    echo
    echo "   ✓ k8pk contexts:"
    k8pk contexts 2>&1 | head -5
else
    echo "   ✗ k8pk not found in PATH"
    echo "   Try: source $RC_FILE or restart iTerm2"
fi

echo
echo "=== Setup Complete ==="
echo
echo "Next steps:"
echo "1. Restart iTerm2 or run: source $RC_FILE"
echo "2. Test: kpick (interactive picker)"
echo "3. Test: kswitch <context> [namespace]"
echo "4. Test: k8pk pick (non-shell-integrated version)"

