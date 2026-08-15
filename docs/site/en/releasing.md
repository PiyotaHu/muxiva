# Release operations

Muxiva publishes each version from one signed Git tag. Pushing the tag starts
the Python release automatically. The CLI workflow is dispatched explicitly
against that same tag after its Homebrew publisher is ready. Both workflows
share a concurrency group, so they cannot race while creating or updating the
same GitHub Release.

## Release channels

| Channel | Output | Supply-chain controls |
| --- | --- | --- |
| CLI | macOS ARM64/Intel, Linux ARM64/x86_64, Windows x86_64 archives | locked Cargo build, native smoke test, SHA-256, GitHub build-provenance attestation |
| Homebrew | release-pinned `muxiva.rb` in `PiyotaHu/homebrew-muxiva` | architecture-specific checksums and an Apple Silicon install test before tap update |
| Python | 28 CPython 3.8–3.14 wheels plus sdist | wheel install tests, SHA-256, provenance, PyPI Trusted Publishing |

GitHub provenance attestation is a cryptographically signed statement about
the workflow and commit that produced an artifact. It is not Apple Developer ID
code signing or notarization, and the project does not claim that it is.

## One-time publisher setup

The canonical names and their confirmation state live in
`release/identity.json`. A release workflow refuses to publish a channel while
its publisher is marked unconfirmed.

### 1. GitHub

The canonical repository is `PiyotaHu/muxiva`. The owner and current repository
administrator were verified on 2026-08-15.

### 2. Homebrew tap

The public repository `PiyotaHu/homebrew-muxiva`, its default branch, and the
GitHub environment named `homebrew` are configured. A dedicated SSH Deploy Key
has write access only to the tap; its private half is stored as the environment
secret `HOMEBREW_TAP_DEPLOY_KEY`. The canonical repository is configured as:

```bash
gh variable set HOMEBREW_TAP_REPOSITORY --body PiyotaHu/homebrew-muxiva
```

The deploy key is verified as writable and `homebrew.confirmed` records the
verification date. A successful release first installs the generated Formula
on a GitHub-hosted M1 runner, then checks out the tap with the scoped key and
commits `Formula/muxiva.rb`. Revoke that single deploy key to disable automated
tap updates without affecting the maintainer account.

### 3. PyPI

The `muxiva` project did not exist on PyPI when checked on 2026-08-15. A pending
Trusted Publisher is now configured for:

- PyPI project: `muxiva`
- GitHub owner: `PiyotaHu`
- repository: `muxiva`
- workflow: `release-python.yml`
- environment: `pypi`

A matching GitHub environment named `pypi` was created and the publisher is
recorded as confirmed in `release/identity.json`. The first successful workflow
run will create the project on PyPI. No long-lived PyPI token is used.

### 4. npm

The public registry reported that the `@muxiva` scope did not exist on
2026-08-15, and this machine had no authenticated npm session. Create or claim
the `muxiva` organization, require 2FA, and verify the owner account with:

```bash
npm login
npm org ls muxiva --json
```

Only then set `npm.confirmed` to `true`. The current work does not publish npm
packages yet; the ownership gate reserves the canonical scope for that later
workflow.

## Preflight and release

Metadata checks are safe to run without publisher credentials:

```bash
python3 scripts/release/check-release-identity.py
```

Require Python publisher ownership before a Python-first public tag:

```bash
python3 scripts/release/check-release-identity.py --channel python
```

Make every package version match the intended tag, run the repository quality
gates, commit the release changes, then create and push the signed tag. This
starts the Python release:

```bash
git tag -s v0.1.1 -m "Muxiva v0.1.1"
git push origin v0.1.1
```

After Homebrew ownership is confirmed, publish CLI and Homebrew from that exact
tag:

```bash
gh workflow run release-cli.yml --ref v0.1.1
```

The workflows build and test before publishing. Do not rerun only the publishing
steps against locally produced artifacts, and never dispatch the CLI workflow
from a branch.

## Verify a published CLI

Download an archive and `SHA256SUMS-cli` from the matching GitHub Release, then run:

```bash
sha256sum --check SHA256SUMS-cli --ignore-missing
gh attestation verify muxiva-v0.1.1-aarch64-apple-darwin.tar.gz --repo PiyotaHu/muxiva
```

macOS users normally use the tested Formula instead:

```bash
brew install PiyotaHu/muxiva/muxiva
muxiva --version
```
