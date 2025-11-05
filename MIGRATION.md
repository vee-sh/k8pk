# Migration Guide: a1ex-var1amov → alex-vee-sh

This document tracks all locations that need to be updated when migrating the repository from `a1ex-var1amov/k8pk` to `alex-vee-sh/k8pk`.

## Files to Update

### Core Configuration Files
- `.github/workflows/release.yml` - GitHub Actions workflows
- `.github/workflows/release-please.yml` - Release automation
- `install.sh` - Installation script (GitHub API URLs, download URLs)
- `README.md` - Installation instructions, repository links
- `CHANGELOG.md` - Release links
- `CONFIG_FILE.md` - Repository references
- `homebrew/Formula/k8pk.rb` - Homebrew formula homepage/URL
- `plugin/init.lua` - WezTerm plugin repository URL

### Documentation
- All markdown files with repository links
- All code comments with repository references

## Commands to Update Repository

```bash
# Update remote URL
git remote set-url origin https://github.com/alex-vee-sh/k8pk.git

# Or if using SSH:
git remote set-url origin git@github.com:alex-vee-sh/k8pk.git

# Verify
git remote -v
```

## Search and Replace Pattern

When ready to migrate, search for:
- `a1ex-var1amov/k8pk` → `alex-vee-sh/k8pk`
- `a1ex-var1amov` → `alex-vee-sh` (in URLs)
- `github.com/a1ex-var1amov` → `github.com/alex-vee-sh`

## Post-Migration Checklist

- [ ] Update all repository URLs in code
- [ ] Update GitHub Actions workflows
- [ ] Update installation script URLs
- [ ] Update README.md and documentation
- [ ] Update Homebrew formula (if submitting to homebrew-core)
- [ ] Update WezTerm plugin URL
- [ ] Test installation script with new URLs
- [ ] Verify GitHub Actions still work
- [ ] Update any external links/bookmarks
- [ ] Announce migration to users (if public)

## Notes

- GitHub will automatically redirect old URLs for a while, but it's best to update proactively
- Release assets will need to be re-downloaded from the new location
- Users with existing installations will need to update their setup

