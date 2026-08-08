# 参与贡献

Muxiva 欢迎聚焦的设计反馈、可复现 Bug、文档、测试与 Pull Request。

## 从这里开始

- 阅读[贡献指南](https://github.com/PiyotaHu/muxiva/blob/main/CONTRIBUTING.md)；
- 遵守[行为准则](https://github.com/PiyotaHu/muxiva/blob/main/CODE_OF_CONDUCT.md)；
- 通过[私密漏洞报告](https://github.com/PiyotaHu/muxiva/security/advisories/new)提交安全问题；
- 使用 [Discussions](https://github.com/PiyotaHu/muxiva/discussions)讨论使用与架构；
- 使用 [Issues](https://github.com/PiyotaHu/muxiva/issues)提交可复现 Bug 与已接受工作。

## 文档契约

公开文档过期时，代码变更不算完成。公开 API、Graph/Manifest Schema、Runtime
语义、Studio、CLI、Node 集成、安全或架构变化，必须同时更新：

```text
docs/site/en/<page>.md
docs/site/zh/<page>.md
```

文档 CI 会检查页面一一对应，并禁止英文 Source 中混入中文正文，然后以严格模式
构建两个语言站点。

## Review 要求

每个 Pull Request 只处理一个逻辑变更。请说明用户结果、保持的 Invariant、兼容
与迁移影响、精确验证命令，以及相关性能或安全风险。
