# Install Voxa / 安装 Voxa

## Application developers / 应用开发者

`voxa` is a binary CLI. Cargo is used once to build and install the current
pre-alpha checkout; it is not part of normal graph execution.

`voxa` 是一个二进制 CLI。当前 pre-alpha 阶段只需使用 Cargo 从源码安装
一次；正常运行 Graph 时不再经过 Cargo。

```bash
git clone https://github.com/PiyotaHu/Voxa.git voxa
cd voxa
cargo install --locked --path crates/voxa-cli

voxa --version
voxa demo
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
voxa demo
voxa init my-agent.voxa.json
voxa validate my-agent.voxa.json
voxa run my-agent.voxa.json
voxa studio my-agent.voxa.json
```

To update a source installation:

更新源码安装：

```bash
git pull --ff-only
cargo install --locked --force --path crates/voxa-cli
```

To remove it:

卸载：

```bash
cargo uninstall voxa-cli
```

## Repository contributors / 仓库贡献者

Commands such as `cargo run -p voxa-cli -- ...` and
`cargo run -p voxa-examples --bin ...` are contributor-only shortcuts for
testing an uninstalled workspace build. User-facing tutorials must use the
installed `voxa` command.

`cargo run -p voxa-cli -- ...` 和 `cargo run -p voxa-examples --bin ...`
只用于贡献者测试尚未安装的 workspace 构建。面向用户的教程必须使用已安装的
`voxa` 命令。

## Public release boundary / 正式发布边界

Before the first public alpha, Voxa still needs signed GitHub binaries with
checksums and platform installers such as Homebrew. Those release channels
will remove the Rust toolchain requirement for application developers. Until
those artifacts exist, the documentation explicitly describes source
installation and does not pretend that a binary release has shipped.

首次公开 Alpha 前还需要提供带校验和及签名的 GitHub 二进制，以及 Homebrew
等平台安装方式。届时应用开发者将不再需要 Rust 工具链。在这些 Artifact 真正
发布之前，文档会明确写作“源码安装”，不会假装已经发布了二进制 Release。
