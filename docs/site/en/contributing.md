# Contributing

Voxa welcomes focused design feedback, reproducible bug reports,
documentation, tests, and pull requests.

## Start here

- read the [contribution guide](https://github.com/PiyotaHu/Voxa/blob/main/CONTRIBUTING.md);
- follow the [Code of Conduct](https://github.com/PiyotaHu/Voxa/blob/main/CODE_OF_CONDUCT.md);
- report vulnerabilities through [Private Vulnerability Reporting](https://github.com/PiyotaHu/Voxa/security/advisories/new);
- use [Discussions](https://github.com/PiyotaHu/Voxa/discussions) for questions and architecture ideas;
- use [Issues](https://github.com/PiyotaHu/Voxa/issues) for reproducible bugs and accepted work.

## Documentation contract

A change is incomplete when its public documentation is stale. Public API,
Graph or Manifest Schema, Runtime semantics, Studio, CLI, provider, security,
or architecture changes must update both:

```text
docs/site/en/<page>.md
docs/site/zh/<page>.md
```

The documentation CI verifies page parity and rejects Chinese prose in English
sources. It then builds both language sites with strict warnings.

## Review expectations

Keep one logical change per pull request. Explain the user outcome, preserved
invariants, compatibility and migration impact, exact verification commands,
and relevant performance or security risks.
