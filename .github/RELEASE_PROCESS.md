# Release Process

This document explains the simplified, straightforward release automation for this project.

## Overview

We use a two-workflow approach:

1. **Release Please** - Manages versioning, changelog, and creates releases
2. **Build Release Binaries** - Builds and uploads platform-specific binaries

## Workflows

### 1. Release Please (`release-please.yml`)

**Purpose**: Automatically manages version bumps, changelog updates, and release creation.

**Triggers**:
- On every push to `main` branch
- Manual dispatch

**What it does**:
- Analyzes commits to determine if a release is needed
- Creates/updates release PRs with version bumps and changelog updates
- When a release PR is merged, automatically creates a GitHub release with tag (e.g., `v0.5.0`)

**Configuration**: `.github/release-please-config.json`

### 2. Build Release Binaries (`release.yml`)

**Purpose**: Builds compiled binaries for all platforms and uploads them to releases.

**Triggers**:
- When a release is published (automatically triggered by Release Please)
- Manual dispatch (with tag input)

**What it does**:
- Builds Rust binaries for:
  - Linux (x86_64)
  - macOS Intel (x86_64)
  - macOS ARM (aarch64)
  - Windows (x86_64)
- Packages each binary with documentation and scripts
- Uploads as `.tar.gz` archives to the GitHub release

## Release Flow

```
1. Developer makes changes and pushes to main
   ↓
2. Release Please workflow runs
   ↓
3. If changes warrant a release, Release Please creates/updates a release PR
   ↓
4. Developer reviews and merges the release PR
   ↓
5. Release Please automatically creates a GitHub release (e.g., v0.5.0)
   ↓
6. Build Release Binaries workflow is triggered by the release event
   ↓
7. Binaries are built for all platforms and uploaded to the release
   ↓
8. Release is complete with binaries available for download
```

## Manual Release

If you need to manually trigger a build for an existing release:

1. Go to Actions → "Build Release Binaries"
2. Click "Run workflow"
3. Enter the tag name (e.g., `v0.5.0`)
4. The workflow will build and upload binaries to that release

The workflow also produces SHA256 checksum files for each archive and uploads them alongside the binaries. Verify downloads with:

```bash
shasum -a 256 k8pk-vX.Y.Z-<target>.tar.gz
cat k8pk-vX.Y.Z-<target>.tar.gz.sha256
```

## What Changed

Previously, the release workflow had:
- Redundant release creation (both Release Please and release.yml tried to create releases)
- Complex tag detection logic
- Unnecessary `create-release` job

Now:
- Release Please handles all release creation
- Build workflow simply builds and uploads binaries when a release is published
- Simpler, cleaner separation of concerns
- No race conditions or conflicts

## Missing Items Check

- ✅ Release creation (handled by Release Please)
- ✅ Version management (handled by Release Please)
- ✅ Changelog updates (handled by Release Please)
- ✅ Binary builds for all platforms (handled by Build Release Binaries)
- ✅ Asset uploads (handled by Build Release Binaries)
- ✅ Manual trigger option (available via workflow_dispatch)

Everything needed for a complete release process is now in place!

