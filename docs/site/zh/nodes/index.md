# Node Package

项目 Node Package 将可执行代码保存在 Graph JSON 之外，`voxa.node/v1`
Manifest 只声明发现与校验元数据。

```text
.voxa/nodes/my_node/
├── voxa.node.json
└── node.py
```

## Manifest 字段

| 字段 | 用途 |
| --- | --- |
| `package_id` | 文件系统安全的项目身份 |
| `display_name` | Palette 中显示的名称 |
| `node_type` | 稳定 Factory 类型名 |
| `language` | `rust`、`cpp`、`python` 或 `typescript` |
| `factory_version` | 精确 Factory 版本 |
| `kind` | `source`、`transform` 或 `sink` |
| `entrypoint` | 语言实现入口 |
| `ports` | 名称、方向与精确 Frame 类型 |
| `config_schema` | Node 配置的 JSON Schema |

## Studio-first 流程

1. 为项目 Graph 打开 Studio。
2. 点击 **Create Node**。
3. 选择语言与 Node 角色。
4. 编辑代码、类型化 Port 与配置 Schema。
5. 点击 **Save & Register**。
6. 对应 Host 可用后，从项目 Palette 添加 Package。
7. 连接兼容 Port 并运行 Graph。

保存不会执行代码。语言 Host 必须先校验并激活 Package，之后它才能进入可运行
Graph。

选择语言指南：

- [Python](python.md)
- [TypeScript](typescript.md)
- [Rust](rust.md)
- [C++](cpp.md)
