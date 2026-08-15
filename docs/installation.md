# Install Muxiva / 安装 Muxiva

## Application developers / 应用开发者

`muxiva` is a binary CLI. After the first public tag, macOS ARM64 and Intel users
install the native release through Homebrew:

```bash
brew install PiyotaHu/muxiva/muxiva
```

Until that tag exists, Cargo is used once to build and install the pre-alpha
checkout; it is not part of normal graph execution.

`muxiva` 是一个二进制 CLI。第一次公开 Tag 发布后，macOS ARM64 与 Intel 用户
通过 Homebrew 安装原生版本：

```bash
brew install PiyotaHu/muxiva/muxiva
```

在该 Tag 存在前，当前 pre-alpha 阶段只需使用 Cargo 从源码安装一次；正常运行
Graph 时不再经过 Cargo。

```bash
git clone https://github.com/PiyotaHu/muxiva.git muxiva
cd muxiva
cargo install --locked --path crates/muxiva-cli

muxiva --version
muxiva
```

The executable is installed to Cargo's binary directory, normally
`$HOME/.cargo/bin`. If the shell cannot find it, load the Rust environment once:

二进制通常安装到 `$HOME/.cargo/bin`。如果当前 shell 找不到它，请加载 Rust
环境：

```zsh
source "$HOME/.cargo/env"
```

After installation, the product commands are:

安装完成后的产品命令是：

```bash
muxiva studio
muxiva init my-agent
muxiva validate my-agent
muxiva run my-agent
muxiva doctor --voice
muxiva simulate --turns 4  # offline Runtime fixture, not a product demo
```

To update a source installation:

更新源码安装：

```bash
git pull --ff-only
cargo install --locked --force --path crates/muxiva-cli
```

To remove it:

卸载：

```bash
cargo uninstall muxiva-cli
```

## Repository contributors / 仓库贡献者

Commands such as `cargo run -p muxiva-cli -- ...` and
`cargo run -p muxiva-examples --bin ...` are contributor-only shortcuts for
testing an uninstalled workspace build. User-facing tutorials must use the
installed `muxiva` command.

`cargo run -p muxiva-cli -- ...` 和 `cargo run -p muxiva-examples --bin ...`
只用于贡献者测试尚未安装的 workspace 构建。面向用户的教程必须使用已安装的
`muxiva` 命令。

## Public release boundary / 正式发布边界

The CLI workflow now builds five native archives, checksums them, generates
GitHub build-provenance attestations, tests the Homebrew Formula on Apple
Silicon, and updates the official tap. The workflow remains intentionally gated
until the tap owner is confirmed. Provenance attestation does not claim Apple
Developer ID signing or notarization.

CLI Workflow 现在会构建五个平台原生压缩包、生成校验和与 GitHub 构建来源证明、
在 Apple Silicon 上测试 Homebrew Formula，并更新官方 Tap。在 Tap Owner 确认前，
Workflow 会主动阻止发布。Provenance Attestation 不等同于 Apple Developer ID
签名或公证，文档不会混淆二者。
