# Repository guidance

Before changing Chromium pins, downstream patches or GN arguments, and before preparing
a Stainless browser build, package or release, load or read the complete
[Chromium update skill](.github/skills/chromium-update/SKILL.md) and follow its workflow.
For check-only or plan-only requests, inspect and report without changing sources.

The baseline is the latest official Mac Stable Chromium version and full revision.
Stale or unavailable upstream evidence cannot pass the release gate. The skill is a
runbook, not a scheduled monitoring service; preserve the release-evidence boundaries in
[the Phase 1 contract](docs/phase-one.md#release-evidence).
