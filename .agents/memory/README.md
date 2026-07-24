# Agent Memory

Read this folder first when working on Winderust.

`AGENTS.md` contains the short non-negotiable contract. This folder holds the details needed to implement changes without rediscovering product decisions.

## Routing

Always read [`00-agent-start.md`](00-agent-start.md) for current decisions and
user constraints. Then read only what the task needs:

- [`10-development-guide.md`](10-development-guide.md) for code, validation,
  contribution, or release work.
- [`15-design-spec.md`](15-design-spec.md) for UI or UX work.
- [`20-project-scope.md`](20-project-scope.md) for product boundaries or future
  direction.
- [`30-reference-library.md`](30-reference-library.md) for Windows APIs and
  operating-system mechanisms.

## Maintenance

- Keep durable current decisions and active user constraints in
  `00-agent-start.md`; remove them when they stop applying.
- Move durable engineering rules into `10-development-guide.md`.
- Put UI design rules in `15-design-spec.md`.
- Put product boundaries in `20-project-scope.md`.
- Put API/reference facts in `30-reference-library.md`.
- Record only durable decisions. Do not copy temporary task notes, test output
  counts, one-off audit results, or speculative future architecture into memory.
