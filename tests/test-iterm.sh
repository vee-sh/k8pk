#!/bin/bash
# Quick test script for iTerm2 after setup

# Get script directory and project root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_ROOT"

echo "=== Testing k8pk in iTerm2 ==="
echo

# Test 1: k8pk command
echo "1. Testing k8pk command..."
if command -v k8pk >/dev/null 2>&1; then
    echo "   ✓ k8pk found: $(which k8pk)"
    k8pk --help | head -3
else
    echo "   ✗ k8pk not found. Run: sudo install -m 0755 $PROJECT_ROOT/rust/k8pk/target/release/k8pk /usr/local/bin/k8pk"
    exit 1
fi
echo

# Test 2: Shell functions
echo "2. Testing shell functions..."
if command -v kpick >/dev/null 2>&1; then
    echo "   ✓ kpick function available"
else
    echo "   ✗ kpick not found. Add to ~/.zshrc: source $PROJECT_ROOT/shell/k8pk.sh"
fi

if command -v kswitch >/dev/null 2>&1; then
    echo "   ✓ kswitch function available"
else
    echo "   ✗ kswitch not found. Add to ~/.zshrc: source $PROJECT_ROOT/shell/k8pk.sh"
fi
echo

# Test 3: List contexts
echo "3. Testing context listing..."
CTX_COUNT=$(k8pk contexts 2>/dev/null | wc -l | tr -d ' ')
if [ "$CTX_COUNT" -gt 0 ]; then
    echo "   ✓ Found $CTX_COUNT contexts"
    echo "   Sample contexts:"
    k8pk contexts 2>&1 | head -3 | sed 's/^/     /'
else
    echo "   ⚠ No contexts found"
fi
echo

# Test 4: Test switch
echo "4. Testing kswitch (dry run)..."
SAMPLE_CTX=$(k8pk contexts 2>&1 | head -1)
if [ -n "$SAMPLE_CTX" ]; then
    echo "   Testing with context: $SAMPLE_CTX"
    k8pk env --context "$SAMPLE_CTX" 2>&1 | head -3 | sed 's/^/     /'
    echo "   ✓ env command works"
else
    echo "   ⚠ No contexts available for testing"
fi
echo

echo "=== Test Complete ==="
echo
echo "Ready to use:"
echo "  kpick              # Interactive picker"
echo "  kswitch <ctx> [ns] # Direct switch"
echo

