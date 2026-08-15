# 发布运维

Muxiva 的每个版本都从同一个签名 Git Tag 发布。推送 Tag 会自动启动 Python
发布；Homebrew 发布方就绪后，再针对同一个 Tag 手动启动 CLI Workflow。CLI 与
Python Workflow 共用 Concurrency Group，因此不会同时创建或修改同一个 GitHub
Release。

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

公开仓库 `PiyotaHu/homebrew-muxiva`、默认分支和名为 `homebrew` 的 GitHub
Environment 已配置。专用 SSH Deploy Key 只对该 Tap 有写权限；其私钥保存在
Environment Secret `HOMEBREW_TAP_DEPLOY_KEY` 中。规范仓库变量配置为：

```bash
gh variable set HOMEBREW_TAP_REPOSITORY --body PiyotaHu/homebrew-muxiva
```

Deploy Key 已验证可写，`release/identity.json` 也记录了确认日期。正式 Release
会先在 GitHub M1 Runner 安装并测试 Formula，再使用该受限密钥 checkout Tap 并
提交 `Formula/muxiva.rb`。撤销这一枚 Deploy Key 即可停止自动更新 Tap，不会影响
维护者账号。

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

Python 优先发布前，只要求 Python 发布方归属已确认：

```bash
python3 scripts/release/check-release-identity.py --channel python
```

确认所有 Package Version 与目标 Tag 相同，跑完仓库质量门禁并提交 Release 改动，
再创建并推送签名 Tag。该操作会启动 Python 发布：

```bash
git tag -s v0.1.1 -m "Muxiva v0.1.1"
git push origin v0.1.1
```

Homebrew 归属确认后，针对同一个 Tag 发布 CLI 与 Homebrew：

```bash
gh workflow run release-cli.yml --ref v0.1.1
```

Workflow 会先构建和测试，再执行发布。不要用本地生成的产物单独重跑发布步骤，也
不要从 Branch 启动 CLI 发布。

## 验证已发布的 CLI

从同一个 GitHub Release 下载压缩包与 `SHA256SUMS-cli`：

```bash
sha256sum --check SHA256SUMS-cli --ignore-missing
gh attestation verify muxiva-v0.1.1-aarch64-apple-darwin.tar.gz --repo PiyotaHu/muxiva
```

macOS 用户通常直接使用已在 M1 Runner 测试过的 Formula：

```bash
brew install PiyotaHu/muxiva/muxiva
muxiva --version
```
