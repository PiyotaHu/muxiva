# Voxa Studio

Studio 是随 `voxa` CLI 发布的本地可视化开发环境。它编辑的就是校验器与
Runtime Compiler 使用的严格 Graph v1 文档，不存在浏览器专用图格式。

## 可视化开发流程

1. 将内置或项目 Node 从 Palette 拖到画布。
2. 从输出 Port 拉线到类型兼容的输入 Port。
3. 选择 Node，编辑标识、Factory 版本和配置。
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

Package 会立即进入当前项目的 Palette。文本 Python Source、Transform 与 Sink
可以通过可信本地开发 Host 运行。TypeScript、Rust 和 C++ 项目 Package 当前可
编辑与保存，但在对应 Build Host 完成前不会进入可运行 Graph。

## Runtime 可观测性

Runtime 面板展示回调次数与耗时、活跃或失败 Node、Edge 吞吐、队列占用、丢帧
以及最终结果。Run 使用当前画布快照，不要求预先保存。

## 安全边界

Studio 默认只监听 `127.0.0.1`，所有接口要求随机 Bearer Token。保存、浏览或
校验 Package 都不会执行源码；只有可信本地用户主动点击 **Run** 后，语言 Host
才会加载 Package。

Studio 不是远程生产控制面，禁止直接暴露到公网。
