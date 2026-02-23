---
name: release
description: Use when the user asks to release, cut a release, bump the version, tag a release, or publish a new version
---

# Release

Bumps version, tags, and pushes to trigger the Docker image release on ghcr.io.

## Process

### 1. Check Preconditions

Verify ALL before proceeding:
- Working tree is clean (`git status`)
- On `main` branch
- Tests pass (`cargo test`)
- Remote is configured (`git remote -v`)

If any fail, STOP and resolve first.

### 2. Determine Version

Ask the user for bump type:
- **patch** (0.1.0 → 0.1.1): bug fixes, dependency updates
- **minor** (0.1.0 → 0.2.0): new features, non-breaking changes
- **major** (0.1.0 → 1.0.0): breaking changes

If the user provided an explicit version (e.g. "release v0.2.0"), use that directly.

### 3. Update Version

Edit `version` in `Cargo.toml`, then `cargo check` to regenerate `Cargo.lock`.

### 4. Commit, Tag, Push

```sh
git add Cargo.toml Cargo.lock
git commit -m "release: v{VERSION}"
git tag "v{VERSION}"
git push && git push --tags
```

Tag must have `v` prefix — the workflow triggers on `v*`.

### 5. Verify

Run `gh run list --limit 1` to confirm the release workflow triggered. Show the user the run URL.

## Common Mistakes

| Mistake | Fix |
|---------|-----|
| Forgetting `Cargo.lock` | Always `cargo check` after edit, stage both files |
| Pushing without tag | Use `git push && git push --tags` together |
| Tag missing `v` prefix | Workflow matches `v*` — use `v0.2.0` not `0.2.0` |
| Dirty working tree | Check `git status` first |
