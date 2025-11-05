#!/bin/bash
# Migration script: a1ex-var1amov/k8pk → alex-vee-sh/k8pk
# Run this script when ready to migrate the repository

set -e

OLD_ORG="a1ex-var1amov"
NEW_ORG="alex-vee-sh"
REPO="k8pk"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Repository Migration Script"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "This will update all references from:"
echo "  github.com/${OLD_ORG}/${REPO}"
echo "to:"
echo "  github.com/${NEW_ORG}/${REPO}"
echo ""
read -p "Are you sure you want to proceed? (yes/no): " confirm

if [ "$confirm" != "yes" ]; then
    echo "Migration cancelled."
    exit 0
fi

echo ""
echo "Updating files..."

# Find and replace in all files
find . -type f \( -name "*.md" -o -name "*.sh" -o -name "*.rb" -o -name "*.lua" -o -name "*.yml" -o -name "*.yaml" -o -name "*.rs" -o -name "*.toml" \) ! -path "./.git/*" ! -path "./target/*" ! -path "./node_modules/*" -exec sed -i '' "s|${OLD_ORG}/${REPO}|${NEW_ORG}/${REPO}|g" {} +

echo "[OK] Updated repository references"
echo ""
echo "Files modified. Please review the changes:"
echo "  git diff"
echo ""
echo "Next steps:"
echo "  1. Review changes: git diff"
echo "  2. Commit: git add -A && git commit -m 'chore: migrate repository to ${NEW_ORG}/${REPO}'"
echo "  3. Update remote: git remote set-url origin https://github.com/${NEW_ORG}/${REPO}.git"
echo "  4. Push: git push origin main"
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

