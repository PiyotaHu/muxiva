# Developing Voxa Nodes

A Voxa Node package keeps executable code outside Graph JSON. Its
`voxa.node/v1` Manifest declares the stable Factory identity, language, role,
typed Ports, configuration Schema, and entrypoint. The project Node Library is
stored beside the Graph under `.voxa/nodes/`.

The preferred authoring journey is Voxa Studio:

1. Open `voxa studio agent.json`.
2. Select **Create Node** in the Node Palette.
3. Choose a language and Source, Transform, or Sink role.
4. Edit the starter, typed Ports, and configuration Schema.
5. Select **Save & Register**.
6. Activate the language execution Host when Studio requests it.
7. Add the registered Node to the canvas and connect compatible Ports.

Saving never inserts executable code into Graph JSON and never executes it.
Execution begins only after the developer explicitly runs the Graph. Project
packages are trusted local code and should be reviewed before activation.

Language guides:

- [Rust](rust.md)
- [Python](python.md)
- [TypeScript](typescript.md)
- [C++](cpp.md)
- [`voxa.node/v1` Manifest](package-manifest.md)

## 中文说明

Voxa Node Package 将代码保存在 Graph JSON 之外。Studio 的 **Create Node**
提供模板、代码编辑器、类型化 Port 和配置 Schema 编辑；**Save & Register**
会把代码与 Manifest 写入当前项目的 `.voxa/nodes/`，并加入项目 Palette。
保存不会执行代码，只有开发者主动 Run Graph 后才会加载受信任的项目代码。
