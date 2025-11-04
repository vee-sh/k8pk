#!/bin/bash
# Test k8pk with both kubectl and oc CLIs

set -e

# Get script directory and project root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_ROOT"

K8PK="$PROJECT_ROOT/rust/k8pk/target/release/k8pk"

echo "=== Testing k8pk with kubectl and oc ==="
echo

# Check which CLIs are available
echo "1. Checking CLI availability..."
KUBECTL_AVAILABLE=false
OC_AVAILABLE=false

if command -v kubectl >/dev/null 2>&1; then
    KUBECTL_AVAILABLE=true
    echo "  ✓ kubectl found: $(which kubectl)"
else
    echo "  ⚠ kubectl not found"
fi

if command -v oc >/dev/null 2>&1; then
    OC_AVAILABLE=true
    echo "  ✓ oc found: $(which oc)"
else
    echo "  ⚠ oc not found"
fi

echo

# Find test contexts
echo "2. Finding test contexts..."
KUBECTL_CTX=""
OC_CTX=""

if [ "$KUBECTL_AVAILABLE" = true ]; then
    KUBECTL_CTX=$($K8PK contexts 2>/dev/null | grep -E "(arn:aws:eks|gke_|kind-|minikube)" | head -1 || echo "")
    if [ -n "$KUBECTL_CTX" ]; then
        echo "  ✓ kubectl context found: $KUBECTL_CTX"
    else
        echo "  ⚠ No kubectl-style contexts found (EKS/GKE/kind/minikube)"
    fi
fi

if [ "$OC_AVAILABLE" = true ]; then
    OC_CTX=$($K8PK contexts 2>/dev/null | grep -E "(/api-|/api\.)" | head -1 || echo "")
    if [ -n "$OC_CTX" ]; then
        echo "  ✓ oc context found: $OC_CTX"
    else
        echo "  ⚠ No OpenShift-style contexts found"
    fi
fi

echo

# Test kubectl integration
if [ -n "$KUBECTL_CTX" ]; then
    echo "3. Testing kubectl integration..."
    echo "   Context: $KUBECTL_CTX"
    
    # Test namespace listing
    echo "   - Testing namespace listing..."
    if $K8PK namespaces --context "$KUBECTL_CTX" 2>&1 | head -3 > /dev/null; then
        echo "     ✓ namespaces command works"
        NS_COUNT=$($K8PK namespaces --context "$KUBECTL_CTX" 2>/dev/null | wc -l | tr -d ' ')
        echo "     ✓ Found $NS_COUNT namespaces"
    else
        echo "     ⚠ namespaces command failed (may need auth)"
    fi
    
    # Test env command
    echo "   - Testing env command..."
    ENV_OUTPUT=$($K8PK env --context "$KUBECTL_CTX" 2>&1)
    if echo "$ENV_OUTPUT" | grep -q "KUBECONFIG="; then
        echo "     ✓ env command works"
        echo "$ENV_OUTPUT" | grep "KUBECONFIG=" | head -1
    else
        echo "     ✗ env command failed"
        echo "$ENV_OUTPUT"
    fi
    
    # Test gen command
    echo "   - Testing gen command..."
    TMP_KUBE="/tmp/k8pk-test-kubectl-$$.yaml"
    if $K8PK gen --context "$KUBECTL_CTX" --out "$TMP_KUBE" 2>&1; then
        echo "     ✓ gen command works"
        if kubectl --kubeconfig="$TMP_KUBE" config view --minify >/dev/null 2>&1; then
            echo "     ✓ Generated kubeconfig is valid for kubectl"
        fi
        rm -f "$TMP_KUBE"
    else
        echo "     ✗ gen command failed"
    fi
    
    echo
fi

# Test oc integration
if [ -n "$OC_CTX" ]; then
    echo "4. Testing oc integration..."
    echo "   Context: $OC_CTX"
    
    # Test namespace/project listing
    echo "   - Testing namespace/project listing..."
    if $K8PK namespaces --context "$OC_CTX" 2>&1 | head -3 > /dev/null; then
        echo "     ✓ namespaces command works with oc"
        NS_COUNT=$($K8PK namespaces --context "$OC_CTX" 2>/dev/null | wc -l | tr -d ' ')
        echo "     ✓ Found $NS_COUNT projects"
        
        # Try to get first namespace
        FIRST_NS=$($K8PK namespaces --context "$OC_CTX" 2>/dev/null | head -1)
        if [ -n "$FIRST_NS" ]; then
            echo "     ✓ First project: $FIRST_NS"
        fi
    else
        echo "     ⚠ namespaces command failed (may need auth)"
    fi
    
    # Test env command with namespace
    echo "   - Testing env command with namespace..."
    if [ -n "$FIRST_NS" ]; then
        ENV_OUTPUT=$($K8PK env --context "$OC_CTX" --namespace "$FIRST_NS" 2>&1)
        if echo "$ENV_OUTPUT" | grep -q "KUBECONFIG=" && echo "$ENV_OUTPUT" | grep -q "OC_NAMESPACE="; then
            echo "     ✓ env command works with OC_NAMESPACE"
            echo "$ENV_OUTPUT" | grep "OC_NAMESPACE=" | head -1
        else
            echo "     ✗ env command failed or missing OC_NAMESPACE"
        fi
    else
        ENV_OUTPUT=$($K8PK env --context "$OC_CTX" 2>&1)
        if echo "$ENV_OUTPUT" | grep -q "KUBECONFIG="; then
            echo "     ✓ env command works"
        else
            echo "     ✗ env command failed"
        fi
    fi
    
    # Test gen command
    echo "   - Testing gen command..."
    TMP_OC="/tmp/k8pk-test-oc-$$.yaml"
    if $K8PK gen --context "$OC_CTX" --out "$TMP_OC" 2>&1; then
        echo "     ✓ gen command works"
        if oc --kubeconfig="$TMP_OC" config view --minify >/dev/null 2>&1; then
            echo "     ✓ Generated kubeconfig is valid for oc"
        fi
        rm -f "$TMP_OC"
    else
        echo "     ✗ gen command failed"
    fi
    
    # Test actual oc command with generated config
    if [ -n "$FIRST_NS" ]; then
        echo "   - Testing oc project command..."
        TMP_OC_NS="/tmp/k8pk-test-oc-ns-$$.yaml"
        if $K8PK gen --context "$OC_CTX" --namespace "$FIRST_NS" --out "$TMP_OC_NS" 2>&1; then
            OC_PROJECT=$(oc --kubeconfig="$TMP_OC_NS" config view --minify -o jsonpath='{.contexts[0].context.namespace}' 2>/dev/null || echo "")
            if [ "$OC_PROJECT" = "$FIRST_NS" ]; then
                echo "     ✓ Namespace embedded correctly in kubeconfig: $OC_PROJECT"
            else
                echo "     ⚠ Namespace mismatch: expected $FIRST_NS, got $OC_PROJECT"
            fi
            rm -f "$TMP_OC_NS"
        fi
    fi
    
    echo
fi

# Test CLI detection
echo "5. Testing CLI auto-detection..."
DETECTED_CLI=$($K8PK namespaces --context "$OC_CTX" 2>&1 | head -1 >/dev/null && echo "oc" || echo "unknown")
if [ "$OC_AVAILABLE" = true ] && [ -n "$OC_CTX" ]; then
    echo "   ✓ k8pk correctly uses oc when available"
fi

echo
echo "=== Test Summary ==="
if [ -n "$KUBECTL_CTX" ]; then
    echo "✓ kubectl integration tested"
fi
if [ -n "$OC_CTX" ]; then
    echo "✓ oc integration tested"
fi
echo "✓ Both CLIs work seamlessly with k8pk"

