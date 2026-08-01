# Graph 与类型化端口

Graph v1 是声明式配置：它通过精确身份选择可信 Node Factory，并用唯一确定的
Frame 类型连接具名 Port。

```json
{
  "version": "voxa.graph/v1",
  "graph_id": "text-agent",
  "nodes": [
    {
      "id": "source",
      "node_type": "builtin.text_source",
      "language": "rust",
      "factory_version": "1.0.0",
      "node_config": {"text": "hello"}
    }
  ],
  "edges": []
}
```

## Factory 身份

Graph 使用下面的三元组解析 Node：

```text
node_type + language + factory_version
```

校验器不会猜测版本，也不会静默选择另一种语言。

## Frame 类型

Port 只能接受以下一种类型：

- `audio`
- `video`
- `text`
- `byte`
- `signal`
- `event`

系统中没有无类型 `any` Port。只有 Source 与 Target Port 类型完全一致时，Edge
才合法。

## Queue Policy

每条 Edge 都有有界 Capacity，以及 `block`、`drop_oldest`、`drop_newest` 或
`abort` 等 Overflow Policy。具体选择取决于业务更看重新鲜度、完整性还是快速
失败。

## 安全限制

Graph JSON 不能包含可执行源码、动态脚本、凭据或任意远程资源；它只能引用可信
Factory 与声明式配置。
