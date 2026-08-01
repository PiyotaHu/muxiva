# Voxa Studio

Studio 是随 `voxa` CLI 发布的本地可视化开发环境。它编辑的就是校验器与
Runtime Compiler 使用的严格 Graph v1 文档，不存在浏览器专用图格式。

## 可视化开发流程

1. 将内置或项目 Node 从 Palette 拖到画布。
2. 从输出 Port 拉线到类型兼容的输入 Port。
3. 选择 Node，查看 Factory 元数据、配置与实现源码。
4. 校验并运行 Graph，观察实时指标，按需停止 Runtime。
5. 将格式化 Graph JSON 原子写回文件。

Studio 根据 Port Schema 自动推导 Edge 的 Frame 类型。audio、video、text、
byte、signal 与 event Port 之间不允许错误连接。

## 在 Studio 中创建 Node

点击 **Create Node**，选择语言和角色，编辑模板代码，声明 Port 与配置 Schema，
然后点击 **Save & Register**。

```text
.voxa/nodes/my_python_node/
├── voxa.node.json
└── node.py
```

Package 会立即进入当前项目的 Palette。Python Node 通过可信本地 Host 运行；
符合 Voxa ABI v1、放在 `.voxa/native/<package_id>/` 的 C++ 动态库也会被严格核对
Manifest 身份、版本、角色与 Port 后加载。TypeScript 与 Rust 项目源码目前仍需
在 Studio 外构建为受支持的运行产物。

选中项目 Node 后，Inspector 会展示 `.voxa/nodes/` 中保存的完整源码，并提供
**Edit in Node Lab**。选中编译内置 Node 时会展示精确 Factory 身份，并链接到
权威 Rust 实现。

## Runtime 可观测性

Runtime 面板展示回调次数与耗时、活跃或失败 Node、Edge 吞吐、队列占用、丢帧
以及最终结果。Run 使用当前画布快照，不要求预先保存。

若项目提供 `.voxa/web/index.html`，工具栏会出现 **Voice Room** 等项目体验入口。
Studio 会先保存当前有效图，再打开项目页面。项目页面只能通过本地 Bearer Token
访问；连接 Manifest 只有显式声明 `client_exposed: true` 的短期字段才能被读取，
其余 API Key、Bot Token 和服务端凭据不会返回浏览器。

## 安全边界

Studio 默认只监听 `127.0.0.1`，所有接口要求随机 Bearer Token。保存、浏览或
校验 Package 都不会执行源码；只有可信本地用户主动点击 **Run** 后，语言 Host
才会加载 Package。

Studio 不是远程生产控制面，禁止直接暴露到公网。
