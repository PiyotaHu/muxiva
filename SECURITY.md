# Security Policy

Voxa processes real-time media and can load native and foreign-language Node
code. Security reports are taken seriously even during pre-alpha development.

## Supported versions

Until the first stable release, security fixes target the latest `main` branch
and the newest published pre-release only. Older commits and unpublished local
builds do not receive backports.

## Report a vulnerability privately

Use [GitHub Private Vulnerability Reporting](https://github.com/PiyotaHu/Voxa/security/advisories/new).
Do not open a public Issue, Discussion, or pull request for an undisclosed
vulnerability.

Include, when possible:

- affected commit, version, platform, language SDK, and feature flags;
- impact and realistic attack preconditions;
- minimal reproduction or proof of concept;
- whether credentials, untrusted Node code, malformed Frames, Graph JSON, FFI,
  RTC callbacks, or shutdown races are involved;
- any suggested mitigation.

We aim to acknowledge a complete report within three business days. Timelines
for validation, remediation, disclosure, and credit depend on severity and
release impact. Please allow a reasonable remediation window before disclosure.

## Security boundaries

- Studio is a trusted local development tool and must not be exposed directly
  to the public internet.
- Graph JSON and Node Manifests are declarative metadata, not authorization to
  execute untrusted source.
- Project Node packages are trusted local code and require an execution Host.
- Credentials and RTC tokens must never be stored in Graph files or committed.
- Native ABI, media buffers, foreign callbacks, cancellation, and shutdown are
  treated as security-sensitive boundaries.

## 中文说明

请通过 GitHub Private Vulnerability Reporting 私密报告漏洞，不要创建公开 Issue。
报告应尽量包含受影响版本、平台、复现方法、攻击前提和影响。Studio 与项目 Node
均属于可信本地开发边界，不能直接用于执行不受信任代码。
