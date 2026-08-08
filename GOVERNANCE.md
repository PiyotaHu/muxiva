# Governance

Muxiva currently uses a maintainer-led, consensus-seeking model appropriate for
an early-stage project.

## Roles

- **Contributors** propose Issues, documentation, tests, designs, and code.
- **Reviewers** have demonstrated expertise in an area and provide technical
  review; review does not imply merge authority.
- **Maintainers** own release, security, repository, and merge decisions.

The current maintainer is [@PiyotaHu](https://github.com/PiyotaHu). Ownership
of sensitive areas is recorded in `.github/CODEOWNERS`.

## Decisions

Routine changes are decided through pull-request review. Changes to public API,
stable ABI, ownership, scheduling, Graph/Manifest schemas, security boundaries,
or governance require an Issue or design document before implementation.

Maintainers seek consensus using technical evidence, tests, compatibility,
operational risk, and the product contract. When consensus cannot be reached,
the maintainer makes and documents the decision. Significant decisions belong
in `docs/design/` and must be published on the documentation site.

## Becoming a reviewer or maintainer

Candidates should demonstrate sustained constructive participation, sound
judgment around safety and compatibility, reliable reviews, and alignment with
the Code of Conduct. Maintainer changes are announced publicly and reflected in
this file and CODEOWNERS.

## 中文说明

Muxiva 当前采用维护者主导、优先寻求共识的治理方式。涉及公开 API、ABI、调度、
所有权、Schema 或安全边界的重大决定，必须先形成 Issue 或设计文档，并同步发布
到文档站。
