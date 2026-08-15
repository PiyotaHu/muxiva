# Release operations

Muxiva publishes each version from one signed Git tag. The CLI and Python
workflows share a concurrency group, so they cannot race while creating or
updating the same GitHub Release.

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

Create the public repository `PiyotaHu/homebrew-muxiva`, add a default branch,
and create a protected GitHub environment named `homebrew`. Grant the workflow a
fine-grained token with **Contents: write** only for the tap repository, and
store it as an environment secret. Configure the main repository without
putting the token in a file:

```bash
gh variable set HOMEBREW_TAP_REPOSITORY --body PiyotaHu/homebrew-muxiva
gh secret set --env homebrew HOMEBREW_TAP_TOKEN
```

Then set `homebrew.confirmed` to `true` in `release/identity.json` and record the
verification date. A successful release first installs the generated Formula on
a GitHub-hosted M1 runner, then commits it to `Formula/muxiva.rb` in the tap.

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

Require all ownership confirmations before the first public tag:

```bash
python3 scripts/release/check-release-identity.py --channel all
```

Make every package version match the intended tag, run the repository quality
gates, commit the release changes, then create and push the signed tag:

```bash
git tag -s v0.1.0 -m "Muxiva v0.1.0"
git push origin v0.1.0
```

The workflows build and test before publishing. Do not rerun only the publishing
steps against locally produced artifacts.

## Verify a published CLI

Download an archive and `SHA256SUMS` from the matching GitHub Release, then run:

```bash
sha256sum --check SHA256SUMS --ignore-missing
gh attestation verify muxiva-v0.1.0-aarch64-apple-darwin.tar.gz --repo PiyotaHu/muxiva
```

macOS users normally use the tested Formula instead:

```bash
brew install PiyotaHu/muxiva/muxiva
muxiva --version
```
