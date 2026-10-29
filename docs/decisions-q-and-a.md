> **Historical record.** This is the raw decision Q&A that seeded the v1 plan. It contains superseded
> answers and one unanswered question. Where it disagrees with [`roadmap.md`](roadmap.md) or an issue
> body, those win.
>
> **Delivery policy superseded on 2026-10-29:** Phase 1 must ship a usable vanilla Chromium browser
> and a downloadable unsigned macOS DMG. Later phases add features to that working browser.
> The answers below prohibiting pre-v1 binaries or requiring release signing before any preview
> are historical, not current gates. This change updates planning only, with no code or CI.

Complete feature Q&A
Q: Who should Stainless optimize for in its first usable release? A: Developers, agent builders, and privacy-conscious/anonymous-browsing users, also benefiting security researchers.
Q: Should development, private browsing, onion browsing, and agent automation use visibly separate isolated modes? A: Yes—explicit modes with separate storage and permissions.
Q: What should be the flagship developer differentiator? A: Primarily an auditable agent workspace for safe browsing, testing, and debugging, plus local AI, enhanced DevTools, ad-free/no-trace browsing, and privacy.
Q: What default permission model should agent workspaces use? A: Full autonomy only inside disposable anonymous sandboxes.
Q: How should work leave an agent sandbox? A: Export reviewed artifacts, reports, patches, and replayable steps; never transfer identity or session state.
Q: What should agent sessions record? A: Configurable levels: structured actions, policy decisions, screenshots, DOM/network/console evidence, replayable XHR, files/state diffs; optional full video; or action/error-only logging.
Q: How should local AI be packaged? A: Provider-neutral adapters, optional bundled local runtime, self-hosted endpoints, and cloud API-key support through OpenAI-compatible endpoints.
Q: How should cloud AI behave in private/onion modes? A: Blocked by default; explicit per-session opt-in with a visible data preview.
Q: What should power stripped-down search? A: User-selected Google, DuckDuckGo, Bing, Ecosia, or self-hosted SearXNG, with AI summaries, recommendations, sponsored results, and similar clutter removed.
Q: How should sanitized results appear? A: A fast clean-results view with “View original” and a removal report.
Q: Which privacy feature should be first-class beyond blocking? A: Disposable identity containers with isolated storage, proxy and fingerprint controls, optional privacy receipts, a user-controlled firewall and kill switch, plus encrypted local/self-hosted profile sync.
Q: How should extension compatibility and stronger sandboxing coexist? A: Dual mode: compatible Chrome extensions and a stricter capability-based Stainless/WASM model.
Q: What should signature DevTools provide? A: Time-travel replay of network/XHR, console, DOM, storage, and agent actions; integrated API testing/mocking/interception/contracts; first-class WASM debugging and profiling.
Q: Should project workspaces bind browser and repository state? A: Yes—optional isolated workspaces containing tabs, DevTools, agents, local servers, environment variables, audit history, and explicitly granted secrets.
Q: How should development tabs differ? A: Immutable tab types with distinct UI, storage, permissions, network policy, and DevTools capabilities.
Q: Initial desktop platforms? A: macOS first, then Linux and Windows.
Q: How should credentials reach users, tabs, and agents? A: Agents use an OS-keychain-backed scoped secret broker without seeing raw secrets. Users get a built-in password/secrets manager with optional existing-manager integration.
Q: Should sessions be branchable? A: Yes—snapshot, fork, compare, discard, or promote development and agent runs.
Q: Should sessions export as reproducible bundles? A: Yes—signed, redacted bundles containing replay steps, logs, traces, mocks, screenshots, and environment metadata.
Q: How strong should onion anonymity be? A: Both a hardened Tor-style mode with standardized fingerprints, letterboxing, isolated circuits, no persistence, and restricted extensions/AI, and a convenient Tor-proxy mode retaining normal features.
Q: What business model? A: Fully free, funded by donations and sponsorships.
Q: What open-source posture? A: No license while private; choose one before public release.
Q: What should the AI interface be? A: A unified dock across browsing, DevTools, and workspaces with cited context and previewed actions.
Q: Should AI retain memory? A: User-curated, encrypted local memory, scoped per profile/workspace and disabled by default.
Q: Which automation interfaces? A: Playwright through a policy-enforcing MCP/agent gateway, plus WebDriver BiDi or raw CDP under policy controls.
Q: Should reusable agent skills be installable? A: Yes—signed, inspectable packages with capability manifests and reproducible versions.
Q: What should the everyday shell prioritize? A: Optional dashboard, familiar Chromium layout, hideable productivity panels, keyboard-first command center, vertical/tree tabs, groups, split panes, and persistent spaces.
Q: Should workspaces include a terminal/process runner? A: Yes—audited, repository-scoped, with explicit host permissions.
Q: Should tools and servers run in containers? A: Yes—Dev Containers/Docker with a native OS sandbox fallback.
Q: Should development tabs include a network lab? A: Yes—interception, latency/offline simulation, proxying, TLS inspection, request mutation, and per-tab audited policies.
Q: Should Stainless provide built-in audits? A: Yes—accessibility, performance, privacy, security headers, visual regression, and Web Vitals in reproducible runs.
Q: Should bundles integrate with issue trackers? A: No external issue-tracker integration.
Q: What should encrypted sync include by default? A: Bookmarks, settings, spaces, and extension manifests. Passwords, workspace state, and audit logs require separate opt-in.
Q: How should audit history be stored? A: Encrypted append-only event storage with per-workspace retention and signed exports.
Q: Which major capability should be deferred beyond v1? A: None; create additional GitHub issues and later assign them to agent swarms if necessary.
Q: How should release channels work? A: Nightly, developer preview, beta, then stable—but no public builds until every planned capability is complete; only one internal build during development.
Q: How should crash reporting work? A: Local diagnostics dashboard and explicit redacted report export; no automatic telemetry.
Q: Should Stainless include a research notebook? A: Yes—encrypted, local-first, profile/workspace scoped, and exportable.
Q: Should research evidence support immutable captures? A: Yes—content-addressed snapshots with WARC/MHTML export and capture metadata.
Q: How should untrusted downloads work? A: Quarantine with provenance, hashes, optional malware scanning, sandboxed preview, and explicit promotion.
Q: How should uploads and clipboard transfers cross isolation boundaries? A: Show destination, content summary, and sensitivity warnings; require one-time approval.
Q: Should disposable identities include aliases? A: Yes—provider-neutral email aliases and masked identities.
Q: What fingerprint strategy should privacy containers use? A: Per-site stable randomized identities; standardized fingerprints in hardened onion mode.
Q: How should site permissions behave? A: One-time/session-scoped by default; persistent grants require explicit choice and remain visible in a capability ledger.
Q: Should workspaces support parallel agents? A: Yes—separate sandboxes and capabilities, shared artifacts, dependency graphs, and a unified audit timeline.
Q: How should agents communicate? A: Through an audited, schema-driven message/artifact bus with no hidden shared state.
Q: Should agent runs have budgets? A: Yes—time, CPU, memory, network, storage, model tokens/cost, and action-count limits.
Q: How should security policies be configured? A: Declarative policy-as-code with a visual editor, versioning, simulation, and signed workspace policies.
Q: Should events export to observability systems? A: Yes—opt-in OpenTelemetry/OTLP with redaction controls.
Q: Should recorded actions become tests? A: Yes—generate editable Playwright/API tests, mocks, fixtures, and assertions from timeline events.
Q: How should broken test selectors be handled? A: Prefer semantic/accessibility selectors and propose reviewed, evidence-backed repairs; never silently self-heal.
Q: Should DevTools edits become workspace patches? A: Yes—persist overrides as diffs, map through source maps, test live, and export patches.
Q: How deeply should Git be integrated? A: A separate monorepo module providing branches, commits, pushes, and PRs; status/diffs through Pierre libraries; scoped edits/patches; explicit approval for commits and remote operations; development sessions only.
Q: How should tabs move between modes? A: Modes are immutable; recreate the tab in a new isolated context and transfer only the URL or approved artifacts.
Q: Which built-in tab modes? A: Standard, Privacy Container, Developer, Agent Sandbox, Tor Proxy, and Hardened Onion, plus custom policy-derived modes.
Q: Should profiles, spaces, workspaces, and modes be distinct? A: Yes—profiles define identity/vault boundaries, spaces organize browsing, workspaces bind projects, and modes enforce tab isolation.
Q: Which proxy identities should containers support? A: VPN, SOCKS, HTTP proxy, and proxy chains with route verification, DNS controls, health checks, and fail-closed kill switches.
Q: Should hardened onion mode support bridges? A: Yes—Tor bridges and pluggable transports with clear trust warnings.
Q: Should onion mode have onion-specific search providers? A: Yes—opt-in, isolated queries, no clearnet fallback, and no cross-mode history.
Q: Should onion mode have security levels? A: Yes—Standard, Safer, and Safest, progressively restricting JavaScript, media, fonts, and risky APIs, with temporary site exceptions.
Q: Should privacy receipts show fingerprint entropy? A: Yes—with explanations, actionable mitigations, and before/after comparisons.
Q: How advanced should native blocking be? A: Full rule debugger/editor, filter lists, cosmetic rules, scriptlets, site policies, and signed updates.
Q: Should DNS be configurable? A: Yes—system DNS, DoH, DoT, ODoH, proxy-resolved DNS, leak tests, and fail-closed policies.
Q: Should TLS controls be exposed? A: Yes—certificate transparency, chain details, pinning alerts, development CAs, and downgrade detection.
Q: How should phishing protection preserve privacy? A: Locally checked signed threat lists with optional privacy-preserving or self-hosted lookups.
Q: How should updates be secured? A: Reproducible builds, signed metadata/artifacts, rollback protection, and a public transparency log before release.
Q: Should remote debugging/agent control exist? A: Local-only by default; optional end-to-end encrypted pairing with short-lived scoped credentials and visible indicators.
Q: Should Stainless have a container-friendly headless runtime? A: Yes—with the same policies, extension sandbox, DevTools, audit logs, and deterministic bundles as desktop.
Q: Should sessions support hermetic replay? A: Yes—optionally virtualize network, time, randomness, locale, and selected browser state while marking nondeterministic inputs.
Q: Should cookie/consent dialogs be automated? A: Yes—prefer reject/minimal consent through auditable rules, with visible outcomes and easy undo.
Q: Should suspicious links open in disposable previews? A: Yes—credential-free preview showing redirects, reputation, permissions, and network summary; configurable and bypassable with a modifier key.
Q: Should clean search generate its own summaries? A: Yes—user-invoked only, model/source labeled, locally generated where possible, and cited.
Q: How should users select AI context? A: Explicit context chips for tabs, DOM regions, DevTools events, files, and notes, with redaction and token previews.
Q: Should AI decisions be reproducible? A: Yes—save model/version, prompts, evidence hashes, tool calls, policies, and outputs with secret redaction.
Q: Should resource controls be visible? A: Optionally expose Chromium’s task manager plus CPU, memory, network, storage, battery impact, pause/hibernate controls, and agent-only hard limits disabled by default.
Q: Should agent limits be mandatory? A: Yes—mandatory generous ceilings with editable policy templates.
Q: Should offline workspaces exist? A: Yes—content-addressed site/API/documentation/research captures and mocks.
Q: How should privacy breakage be handled? A: Temporary compatibility mode identifying the blocked capability, showing the privacy delta, and automatically expiring exceptions.
Q: Should there be a privacy panic action? A: Yes—stop agents/network, close selected modes, clear ephemeral data, lock vaults, and optionally rotate identities.
Q: Should there be a screen-sharing privacy mode? A: Yes—mask sensitive tabs, fields, URLs, notifications, DevTools secrets, and AI context.
Q: Should users control agent sessions live? A: Yes—watch, pause, step, branch, take over, inspect policy explanations, checkpoint, and safely hand control back.
Q: Should agents have dry-run mode? A: Yes—simulate against captured state, flag uncertainty, and require explicit transition to execution.
Q: What should WASM extensions support? A: Capability-scoped DevTools panels, search transforms, privacy analyzers, and workspace tools using deterministic, auditable APIs.
Q: Should sanitization extend beyond search? A: Primarily search, but also auditable site-specific transforms with original-view toggles and user rules, such as removing sponsored social content.
Q: Should social feeds offer chronological mode? A: Yes—remove sponsored/recommended modules and show transformation details.
Q: Should Stainless provide private translation? A: Yes—on-device/local-AI first, with explicit Chromium or cloud fallback and source-text previews.
Q: Should it provide captions/transcription/read-aloud? A: Yes—local-first, defaulting to NVIDIA Nemotron 3.5 ASR Streaming 0.6B, with downloadable alternatives such as Whisper, per-site controls, and transcript export.
Q: Should there be a local model manager? A: Yes—signed manifests, checksums, license/size disclosure, hardware compatibility, version pinning, and rollback.
Q: Which macOS inference backends first? A: MLX, llama.cpp, ONNX Runtime, and Core ML through a common adapter.
Q: What should the vault manage? A: Passwords, passkeys, TOTP, API tokens, secure notes, client certificates, and SSH keys, hardware-backed where available.
Q: How should vault/sync recovery work? A: User-held recovery key plus trusted-device approval, optional split recovery shares, and no server-side reset.
Q: Should DevTools compare performance branches/builds? A: Yes—diff Web Vitals, traces, network, memory, CPU, energy, and bundle behavior against regression budgets.
Q: Should DevTools identify supply-chain risk? A: Yes—package/source mapping, SBOM, licenses, vulnerabilities, provenance, and runtime behavior.
Q: Should runtime packages be correlated with repository manifests? A: No repository dependency analysis.
Q: Which API protocols should be supported? A: HTTP/REST, OpenAPI, GraphQL, WebSocket, Server-Sent Events, and gRPC-Web.
Q: Which API collection formats? A: OpenAPI, Postman, Insomnia, Bruno, HAR, and curl, preserving secrets through vault references.
Q: How much code editing should Stainless provide? A: A focused Monaco editor for patches, generated tests, mocks, policies, and small edits—not a full IDE.
Q: Should source locations open in external editors? A: Yes—configurable VS Code, JetBrains, Zed, Vim, and custom URI/CLI integrations.
Q: Should project workspaces include a local-service dashboard? A: Unanswered—the session was aborted before the user responded.
