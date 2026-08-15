# 发布运维

Muxiva 的每个版本都从同一个签名 Git Tag 发布。CLI 与 Python Workflow 共用
Concurrency Group，因此不会同时创建或修改同一个 GitHub Release。

## 发布通道

| 通道 | 产物 | 供应链控制 |
| --- | --- | --- |
| CLI | macOS ARM64/Intel、Linux ARM64/x86_64、Windows x86_64 压缩包 | Cargo Lockfile、原生冒烟测试、SHA-256、GitHub 构建来源证明 |
| Homebrew | 发布版本固定的 `muxiva.rb`，写入 `PiyotaHu/homebrew-muxiva` | 分架构校验和；更新 Tap 前在 M1 Runner 真实安装测试 |
| Python | 28 个 CPython 3.8–3.14 Wheel 与 sdist | Wheel 安装测试、SHA-256、Provenance、PyPI Trusted Publishing |

GitHub Provenance Attestation 是关于产物生成 Workflow 和 Commit 的加密签名
声明，并不等同于 Apple Developer ID 签名或公证；项目不会混淆这两个概念。

## 一次性发布方配置

规范名称及其确认状态记录在 `release/identity.json`。某个发布方未确认时，
对应 Workflow 会拒绝发布，避免形成只有部分渠道可用的公开版本。

### 1. GitHub

规范仓库是 `PiyotaHu/muxiva`。2026-08-15 已确认当前 GitHub 登录账号拥有该公开
仓库的 `ADMIN` 权限。

### 2. Homebrew Tap

创建公开仓库 `PiyotaHu/homebrew-muxiva` 和默认分支，再创建名为 `homebrew` 的受
保护 GitHub Environment。创建只对 Tap 仓库拥有 **Contents: write** 权限的
Fine-grained Token，并把它保存为 Environment Secret。以下配置不会把 Token 写入
文件：

```bash
gh variable set HOMEBREW_TAP_REPOSITORY --body PiyotaHu/homebrew-muxiva
gh secret set --env homebrew HOMEBREW_TAP_TOKEN
```

完成后把 `release/identity.json` 的 `homebrew.confirmed` 改为 `true`，并记录验证
日期。正式 Release 会先在 GitHub M1 Runner 安装并测试 Formula，再把它提交到
Tap 的 `Formula/muxiva.rb`。

### 3. PyPI

2026-08-15 检查时，PyPI 上不存在 `muxiva` 项目。目前已经配置以下 Pending
Trusted Publisher：

- PyPI Project：`muxiva`
- GitHub Owner：`PiyotaHu`
- Repository：`muxiva`
- Workflow：`release-python.yml`
- Environment：`pypi`

对应的 GitHub `pypi` Environment 已创建，Publisher 也已经在
`release/identity.json` 中标记为已确认。第一次成功运行 Workflow 时会在 PyPI
创建项目；发布过程不使用长期 PyPI Token。

### 4. npm

2026-08-15 公共 Registry 返回 `@muxiva` Scope 不存在，本机也没有 npm 登录会话。
请先创建或认领 `muxiva` Organization，强制启用 2FA，再验证 Owner：

```bash
npm login
npm org ls muxiva --json
```

确认后才能把 `npm.confirmed` 改为 `true`。当前工作尚不发布 npm Package；这个
门禁是为后续 Workflow 锁定规范 Scope。

## 预检与发布

不需要发布凭据即可检查全部元数据：

```bash
python3 scripts/release/check-release-identity.py
```

第一次公开发布前，要求全部归属已确认：

```bash
python3 scripts/release/check-release-identity.py --channel all
```

确认所有 Package Version 与目标 Tag 相同，跑完仓库质量门禁并提交 Release 改动，
再创建并推送签名 Tag：

```bash
git tag -s v0.1.0 -m "Muxiva v0.1.0"
git push origin v0.1.0
```

Workflow 会先构建和测试，再执行发布。不要用本地生成的产物单独重跑发布步骤。

## 验证已发布的 CLI

从同一个 GitHub Release 下载压缩包与 `SHA256SUMS`：

```bash
sha256sum --check SHA256SUMS --ignore-missing
gh attestation verify muxiva-v0.1.0-aarch64-apple-darwin.tar.gz --repo PiyotaHu/muxiva
```

macOS 用户通常直接使用已在 M1 Runner 测试过的 Formula：

```bash
brew install PiyotaHu/muxiva/muxiva
muxiva --version
```
