#!/bin/bash
# Quick test script for k8pk

set -e

echo "=== k8pk Test Suite ==="
echo

# Get script directory and project root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_ROOT"

# Check if k8pk is built
if [ ! -f "rust/k8pk/target/release/k8pk" ]; then
    echo "Building k8pk..."
    cd rust/k8pk
    cargo build --release
    cd "$PROJECT_ROOT"
fi

K8PK_BIN="$PROJECT_ROOT/rust/k8pk/target/release/k8pk"

echo "1. Testing context listing..."
$K8PK_BIN contexts || echo "⚠ No contexts found (this is OK if kubectl/oc not configured)"
echo

echo "2. Testing current context..."
$K8PK_BIN current || echo "⚠ No current context"
echo

echo "3. Testing env command (first context)..."
CTX=$($K8PK_BIN contexts 2>/dev/null | head -1)
if [ -n "$CTX" ]; then
    echo "   Using context: $CTX"
    $K8PK_BIN env --context "$CTX" || echo "⚠ Failed"
else
    echo "   ⚠ No contexts available for testing"
fi
echo

echo "4. Testing namespace listing (if context available)..."
if [ -n "$CTX" ]; then
    $K8PK_BIN namespaces --context "$CTX" 2>/dev/null | head -5 || echo "   ⚠ No namespaces or context not accessible"
else
    echo "   ⚠ No context available"
fi
echo

echo "5. Testing gen command..."
if [ -n "$CTX" ]; then
    TMP_KUBE="/tmp/k8pk-test-$$.yaml"
    $K8PK_BIN gen --context "$CTX" --out "$TMP_KUBE" && {
        echo "   ✓ Generated kubeconfig: $TMP_KUBE"
        rm -f "$TMP_KUBE"
    } || echo "   ⚠ Failed to generate"
else
    echo "   ⚠ No context available"
fi
echo

echo "6. Testing which CLI is detected..."
if command -v oc >/dev/null 2>&1; then
    echo "   ✓ oc found: $(which oc)"
elif command -v kubectl >/dev/null 2>&1; then
    echo "   ✓ kubectl found: $(which kubectl)"
else
    echo "   ⚠ Neither oc nor kubectl found"
fi
echo

echo "7. Testing shell integration..."
if [ -f "$PROJECT_ROOT/shell/k8pk.sh" ]; then
    echo "   ✓ Shell script exists"
    source "$PROJECT_ROOT/shell/k8pk.sh" 2>/dev/null && {
        if command -v kpick >/dev/null 2>&1; then
            echo "   ✓ kpick function loaded"
        fi
        if command -v kswitch >/dev/null 2>&1; then
            echo "   ✓ kswitch function loaded"
        fi
    } || echo "   ⚠ Failed to source shell script"
else
    echo "   ⚠ Shell script not found"
fi
echo

echo "=== Test Summary ==="
echo "Run interactive picker: $K8PK_BIN pick"
echo "Run with context: $K8PK_BIN env --context <your-context>"
echo "See TESTING.md for full test plan"

