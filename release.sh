#!/bin/bash
# k8pk release script
# Usage: ./release.sh [version] [message]
# Example: ./release.sh 0.10.13 "Fix something important"

set -e  # Exit on error

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Get version from argument or prompt
if [ -z "$1" ]; then
    CURRENT_VERSION=$(grep '^version = ' rust/k8pk/Cargo.toml | sed 's/version = "\(.*\)"/\1/')
    echo -e "${YELLOW}Current version: ${CURRENT_VERSION}${NC}"
    read -p "Enter new version (e.g., 0.10.13): " NEW_VERSION
else
    NEW_VERSION="$1"
fi

# Validate version format
if ! echo "$NEW_VERSION" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+$'; then
    echo -e "${RED}Error: Invalid version format. Use semantic versioning (e.g., 0.10.13)${NC}"
    exit 1
fi

# Get release message
if [ -z "$2" ]; then
    read -p "Enter release message (or press Enter for default): " RELEASE_MSG
    if [ -z "$RELEASE_MSG" ]; then
        RELEASE_MSG="v${NEW_VERSION}: Release"
    fi
else
    RELEASE_MSG="$2"
fi

echo -e "${GREEN}Preparing release v${NEW_VERSION}...${NC}"

# Step 1: Update Cargo.toml version
echo -e "${YELLOW}Step 1: Updating Cargo.toml...${NC}"
sed -i '' "s/^version = \".*\"/version = \"${NEW_VERSION}\"/" rust/k8pk/Cargo.toml
echo -e "${GREEN}✓ Updated Cargo.toml to version ${NEW_VERSION}${NC}"

# Step 2: Update Cargo.lock (this is the critical step!)
echo -e "${YELLOW}Step 2: Updating Cargo.lock...${NC}"
cd rust/k8pk
cargo check --quiet 2>&1 || {
    echo -e "${YELLOW}Running cargo update to refresh dependencies...${NC}"
    cargo update --quiet || true
}
cargo check --quiet
cd ../..
echo -e "${GREEN}✓ Cargo.lock updated${NC}"

# Step 3: Verify everything compiles
echo -e "${YELLOW}Step 3: Verifying build...${NC}"
cd rust/k8pk
cargo fmt --check --quiet || {
    echo -e "${YELLOW}Formatting code...${NC}"
    cargo fmt
}
cargo clippy --quiet -- -D warnings || {
    echo -e "${RED}Error: Clippy checks failed${NC}"
    exit 1
}
cargo test --quiet || {
    echo -e "${RED}Error: Tests failed${NC}"
    exit 1
}
cd ../..
echo -e "${GREEN}✓ All checks passed${NC}"

# Step 4: Commit changes
echo -e "${YELLOW}Step 4: Committing changes...${NC}"
git add rust/k8pk/Cargo.toml rust/k8pk/Cargo.lock
# Include CHANGELOG if updated for this release
git add CHANGELOG.md 2>/dev/null || true
git status --short
git commit -m "chore: bump version to ${NEW_VERSION}" || {
    echo -e "${YELLOW}No changes to commit (version may already be set)${NC}"
}

# Step 5: Create and push tag
echo -e "${YELLOW}Step 5: Creating tag v${NEW_VERSION}...${NC}"
git tag -d "v${NEW_VERSION}" 2>/dev/null || true  # Delete local tag if exists
git tag -a "v${NEW_VERSION}" -m "${RELEASE_MSG}"

# Step 6: Push everything
echo -e "${YELLOW}Step 6: Pushing to GitHub...${NC}"
git push origin main
git push origin "v${NEW_VERSION}" --force

echo -e "${GREEN}"
echo "╔═══════════════════════════════════════╗"
echo "║   Release v${NEW_VERSION} Created!   ║"
echo "╚═══════════════════════════════════════╝"
echo -e "${NC}"
echo "Tag: v${NEW_VERSION}"
echo "Message: ${RELEASE_MSG}"
echo ""
echo "CI will automatically:"
echo "  - Run all checks"
echo "  - Create GitHub release"
echo "  - Build binaries for all platforms"
echo "  - Update Homebrew tap"
echo ""
echo -e "${GREEN}Done! 🚀${NC}"

