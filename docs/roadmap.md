# Stainless roadmap

Stainless is a free, Chromium-based, developer-first browser combining auditable agent automation,
enhanced DevTools, optional local AI, clean browsing, and explicit identity isolation.

This document is the narrative source of truth for the v1 programme. Authority is split as follows:

- **Issue bodies** own scope, acceptance criteria, and non-goals for each capability.
- **Milestones** own wave membership.
- **This document** owns the narrative, the global decisions, and the resolved ambiguities.
- [`decisions-q-and-a.md`](decisions-q-and-a.md) is a historical decision log; where it conflicts with
  this roadmap or an issue body, this roadmap wins.

Issue [#1](https://github.com/mbianchidev/stainless/issues/1) is the top-level epic and parents every
implementation issue listed below.

## Global decisions

- Local-first, no automatic telemetry, and no required account or cloud AI.
- Profiles are identity/vault boundaries; spaces never mix profiles.
- Tab modes are immutable and transfers carry no implicit identity state.
- Agent autonomy is limited to disposable policy-controlled sandboxes.
- Hardened Onion is persistent-state-free and cannot access extensions, vaults, AI, agents,
  repositories, or local services.
- Search sanitization runs locally after direct provider fetches.
- Sync servers store ciphertext only and cannot reset keys.
- Porto owns local-service lifecycle; Stainless provides an optional read-only adapter.
- Public distribution begins only after the complete v1 scope and security gates pass.
- Chromium-native sandboxing is the v1 baseline; capability-specific OS sandbox enhancements are post-v1.
- Performance is observable but not a numerical v1 release gate.

## Global non-goals

Email, chat, office suites, cloud storage, hosting/deployment, service lifecycle, live multi-user
collaboration, a full IDE, repository dependency analysis, built-in scheduling, hosted search
proxying, mandatory AI, mobile v1, automatic telemetry, and pre-v1 binary distribution.

## Delivery waves

Waves are completion buckets, not strictly serial phases. Dependencies exist inside a wave, and
security, documentation, and release engineering run continuously even though their final acceptance
gates land in Wave 7. No public build is distributed until every v1 issue passes.

| Wave | Theme | Issues |
| --- | --- | --- |
| 0 | Contract | [#2](https://github.com/mbianchidev/stainless/issues/2) |
| 1 | Foundation | [#3](https://github.com/mbianchidev/stainless/issues/3), [#4](https://github.com/mbianchidev/stainless/issues/4), [#5](https://github.com/mbianchidev/stainless/issues/5) |
| 2 | Cross-cutting platform | [#6](https://github.com/mbianchidev/stainless/issues/6), [#7](https://github.com/mbianchidev/stainless/issues/7), [#9](https://github.com/mbianchidev/stainless/issues/9), [#10](https://github.com/mbianchidev/stainless/issues/10), [#11](https://github.com/mbianchidev/stainless/issues/11), [#28](https://github.com/mbianchidev/stainless/issues/28), [#30](https://github.com/mbianchidev/stainless/issues/30) |
| 3 | Usable private browser | [#8](https://github.com/mbianchidev/stainless/issues/8), [#12](https://github.com/mbianchidev/stainless/issues/12), [#13](https://github.com/mbianchidev/stainless/issues/13), [#14](https://github.com/mbianchidev/stainless/issues/14), [#29](https://github.com/mbianchidev/stainless/issues/29) |
| 4 | Extensibility and workspaces | [#15](https://github.com/mbianchidev/stainless/issues/15), [#16](https://github.com/mbianchidev/stainless/issues/16), [#17](https://github.com/mbianchidev/stainless/issues/17), [#18](https://github.com/mbianchidev/stainless/issues/18) |
| 5 | DevTools and automation | [#19](https://github.com/mbianchidev/stainless/issues/19), [#20](https://github.com/mbianchidev/stainless/issues/20), [#21](https://github.com/mbianchidev/stainless/issues/21), [#22](https://github.com/mbianchidev/stainless/issues/22), [#31](https://github.com/mbianchidev/stainless/issues/31) |
| 6 | Intelligence and knowledge | [#23](https://github.com/mbianchidev/stainless/issues/23), [#24](https://github.com/mbianchidev/stainless/issues/24) |
| 7 | Release readiness | [#25](https://github.com/mbianchidev/stainless/issues/25), [#26](https://github.com/mbianchidev/stainless/issues/26), [#27](https://github.com/mbianchidev/stainless/issues/27) |

## Programme structure

| Area | Issues |
| --- | --- |
| Product contract | #2 |
| Chromium and architecture | #3, #4 |
| Build and releases | #5, #6 |
| Browser UX, identities, resources | #7, #8, #29 |
| Privacy and anonymity | #9, #13, #14, #28, #30 |
| Trust and data | #10, #11, #12 |
| Extensions | #15, #16 |
| Developer platform | #17, #18, #19, #20 |
| Agents and automation | #21, #22, #31 |
| AI and research | #23, #24 |
| Shipping | #25, #26, #27 |

## Ownership boundaries

Every v1 capability has exactly one primary owner issue. The boundaries most likely to be confused:

- #19 owns workspace development CAs and active TLS interception; #28 owns passive certificate,
  Certificate Transparency, pinning, and downgrade controls.
- #8 owns the dashboard shell and tab hibernation UX; #29 owns resource measurement, attribution,
  and manual pause/terminate controls, and supplies the extension metrics #15 consumes.
- #9 owns entropy reporting and privacy receipts; #30 owns randomized identity generation and
  rotation; #14 keeps standardized fingerprints for Hardened Onion.
- #21 owns agent runtime policy and mandatory ceilings; #31 owns the skill package format and its
  install/update/revocation lifecycle, reusing the registry infrastructure from #16.

## Resolved conflicts and ambiguities

- **Local-service dashboard:** Porto owns lifecycle; Stainless offers an optional read-only adapter.
- **Built-in modes:** Seven tab modes (Standard, Incognito, Privacy Container, Developer, Agent
  Sandbox, Tor Proxy, Hardened Onion), plus Guest as a separate memory-only profile.
- **Tor management:** Hardened Onion bundles a pinned Tor daemon; Tor Proxy may use bundled or
  user-managed Tor.
- **Leak testing:** Only DNS/route verification is user-facing; comprehensive isolation and
  fingerprint tests remain CI-only.
- **AI in Tor:** Hardened Onion prohibits AI. Tor Proxy blocks cloud AI by default with explicit
  per-session opt-in.
- **Native sandboxing:** Chromium sandboxing is the v1 baseline; capability-specific OS sandbox
  profiles are post-v1.
- **Channels versus no public builds:** Channel behaviour is implemented and tested locally, but no
  binaries are distributed until complete v1 readiness.
- **Collaboration:** Live multi-user browsing/debugging is excluded; same-user observation and
  takeover of an agent session remains in scope.
- **Models:** Arbitrary local weights are allowed; repositories requiring executable custom code
  need separate approval and isolation.
- **CAPTCHAs:** Agents choose strategy, but solvers, stealth changes, payments, and human handoff
  remain policy-gated and audited.
- **Resource limits:** Hard limits for ordinary browsing are disabled by default; agent ceilings are
  mandatory and generous.
- **Accessibility and performance:** Chromium accessibility is the initial baseline; numerical
  performance and reliability thresholds are not v1 release gates.
- **Repository analysis:** Runtime JS/WASM supply-chain inspection is in scope; repository
  manifest/lockfile dependency analysis is excluded.
- **License and disclosure:** Neither is selected yet; both are explicit decisions required before
  public release.
- **Staged rollout without telemetry:** Health-based pauses must avoid browsing telemetry; prefer
  publisher/manual security signals or separately consented minimal updater health data.
- **Distinct user agent:** Stainless identifies distinctly, with visible, expiring per-site
  Chrome-compatible overrides for compatibility failures.

## Working conventions

- Labels: `area:*` identifies the owning area of each issue.
- Milestones: `Wave N - <theme>` identifies delivery wave membership.
- Every implementation issue is a sub-issue of the epic and states its dependencies by issue number.
