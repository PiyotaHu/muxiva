# Install Muxiva / 安装 Muxiva

## Application developers / 应用开发者

`muxiva` is a binary CLI. Cargo is used once to build and install the current
pre-alpha checkout; it is not part of normal graph execution.

`muxiva` 是一个二进制 CLI。当前 pre-alpha 阶段只需使用 Cargo 从源码安装
一次；正常运行 Graph 时不再经过 Cargo。

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

Before the first public alpha, Muxiva still needs signed GitHub binaries with
checksums and platform installers such as Homebrew. Those release channels
will remove the Rust toolchain requirement for application developers. Until
those artifacts exist, the documentation explicitly describes source
installation and does not pretend that a binary release has shipped.

首次公开 Alpha 前还需要提供带校验和及签名的 GitHub 二进制，以及 Homebrew
等平台安装方式。届时应用开发者将不再需要 Rust 工具链。在这些 Artifact 真正
发布之前，文档会明确写作“源码安装”，不会假装已经发布了二进制 Release。
