# Muxiva documentation sources

The public documentation site is strictly bilingual:

```text
docs/site/en/   # English public documentation
docs/site/zh/   # Simplified Chinese public documentation
```

Every public page must exist at the same relative path in both directories.
Run `python scripts/check-docs-i18n.py` and `mkdocs build --strict` before
submitting a documentation change.

Architecture specifications and implementation records remain under
`docs/design/` and `docs/pre_release_notes/`. When one of those documents
changes a public contract or support boundary, update the corresponding paired
page under `docs/site/` in the same pull request.

Do not add mixed-language summaries to a public page. Use the language switcher
to move between complete translations.
