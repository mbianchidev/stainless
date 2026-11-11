# Stainless roadmap

Stainless is a free, Chromium-based, developer-first browser project combining auditable agent
automation, enhanced DevTools, optional local AI, clean browsing, and explicit identity isolation.

**Current state: Phase 1 implementation in progress.** Local build/release tooling, a pinned
Chromium patch stack and a native macOS updater are present. Private preview candidates build,
install/eject and browse locally; separate clean-Mac and actual authorized automatic-update
evidence remain open. The six-patch source baseline removes built-in Google services and
records [the resulting replacement gaps](degoogling.md), not implemented private substitutes. See the
[baseline contract and outstanding gates](phase-one.md) and [build guide](build-macos.md).
The feature/phase descriptions below are requirements, not claims that a phase has shipped.

Issue [#1](https://github.com/mbianchidev/stainless/issues/1) is the roadmap and top-level v1 epic.
Authority is split as follows:

- **The epic** owns the step-by-step execution checklist and phase-release status.
- **Capability issue bodies** own detailed scope, acceptance criteria, dependencies, and non-goals.
- **Milestones** identify the primary delivery phase, not a prohibition on earlier scoped slices.
- **This document** owns the delivery narrative, global decisions, and ownership boundaries.
- [`decisions-q-and-a.md`](decisions-q-and-a.md) is historical; current decisions here and in the
  epic supersede conflicting answers.

## Delivery decision

The 2026-10-29 direction replaces the old all-or-nothing delivery waves:

- Phase 1 must compile, build, bundle, and deliver a usable vanilla Chromium browser with
  automated unsigned updates. Manual download-and-replace is a recovery fallback, not the updater.
- Every phase ends with a downloadable, installable browser that retains earlier working features.
- Private previews may ship before the complete v1 feature set. Keep the repository private;
  public publication still needs the license and disclosure decisions in #27.
- Developer ID app/DMG signing and Apple notarization are deferred. They do not block previews.
  Any local/ad-hoc signatures required to execute Apple Silicon binaries are a build detail, not
  publisher authentication.
- Update payloads and feed metadata may also be unsigned initially. Use validated HTTPS and
  final-asset hashes; document their weaker trust model rather than claiming signed-update security.
- Local build, test, packaging, and release evidence is sufficient initially. No CI, including
  dormant workflow templates, is required for the first preview.
- The full original capability set remains the v1 destination. Features arrive incrementally;
  incomplete capabilities stay unavailable rather than becoming nonfunctional UI.

## Global decisions

- Free, donation/sponsorship funded, with no ads, sponsored placements, or behavioral tracking.
- Local-first, no automatic telemetry, and no required account or cloud AI.
- Profiles are identity/vault boundaries; spaces never mix profiles.
- Tab modes are immutable and transfers carry no implicit identity state.
- Agent autonomy is limited to disposable policy-controlled sandboxes.
- Hardened Onion is persistent-state-free and cannot access extensions, vaults, AI, agents,
  repositories, or local services.
- Search sanitization runs locally after direct provider fetches.
- Sync servers store ciphertext only and cannot reset keys.
- Porto owns local-service lifecycle; Stainless provides an optional read-only adapter.
- Chromium-native sandboxing is the baseline. Capability-specific OS sandbox enhancements remain
  post-v1; no phase may disable Chromium sandboxing to get an app running.
- Performance is observable but not a numerical v1 release gate.
- Claims follow implementation: Phase 1 inherits Chromium behavior, not the later guarantees of
  encrypted profiles, custom containers, fingerprint protection, Tor, or agents.

## Global non-goals

Email, chat, office suites, cloud storage, hosting/deployment, service lifecycle, live multi-user
collaboration, a full IDE, repository dependency analysis, built-in scheduling, hosted search
proxying, mandatory AI, mobile v1, and automatic telemetry.

## Delivery phases

Steps within a phase follow the dependencies stated below; independent work may proceed in
parallel. Research and later integrations cannot hold the first browser hostage.

| Phase | Usable browser at the end | Primary issues |
| --- | --- | --- |
| 1 | Vanilla Chromium on Apple Silicon macOS, unsigned DMG and automated unsigned updates | [#2](https://github.com/mbianchidev/stainless/issues/2), [#3](https://github.com/mbianchidev/stainless/issues/3), [#5](https://github.com/mbianchidev/stainless/issues/5), [#36](https://github.com/mbianchidev/stainless/issues/36), [#37](https://github.com/mbianchidev/stainless/issues/37); updater slice [#6](https://github.com/mbianchidev/stainless/issues/6) |
| 2 | Everyday browser with separate identities, better tabs, recovery, and resource visibility | [#4](https://github.com/mbianchidev/stainless/issues/4), [#7](https://github.com/mbianchidev/stainless/issues/7), [#8](https://github.com/mbianchidev/stainless/issues/8), [#29](https://github.com/mbianchidev/stainless/issues/29); optional research [#33](https://github.com/mbianchidev/stainless/issues/33) |
| 3 | Explainable privacy controls, audit evidence, and a usable credential vault | [#10](https://github.com/mbianchidev/stainless/issues/10), [#9](https://github.com/mbianchidev/stainless/issues/9), [#11](https://github.com/mbianchidev/stainless/issues/11), [#28](https://github.com/mbianchidev/stainless/issues/28), [#30](https://github.com/mbianchidev/stainless/issues/30) |
| 4 | Clean search, optional encrypted sync, and separately gated Tor modes | [#12](https://github.com/mbianchidev/stainless/issues/12), [#13](https://github.com/mbianchidev/stainless/issues/13), [#14](https://github.com/mbianchidev/stainless/issues/14) |
| 5 | Compatible extensions and trusted developer workspaces | [#15](https://github.com/mbianchidev/stainless/issues/15), [#16](https://github.com/mbianchidev/stainless/issues/16), [#17](https://github.com/mbianchidev/stainless/issues/17), [#18](https://github.com/mbianchidev/stainless/issues/18) |
| 6 | Enhanced DevTools, disposable agents, installable skills, and headless automation | [#19](https://github.com/mbianchidev/stainless/issues/19), [#20](https://github.com/mbianchidev/stainless/issues/20), [#21](https://github.com/mbianchidev/stainless/issues/21), [#22](https://github.com/mbianchidev/stainless/issues/22), [#31](https://github.com/mbianchidev/stainless/issues/31) |
| 7 | Optional local AI and an encrypted offline research environment | [#23](https://github.com/mbianchidev/stainless/issues/23), [#24](https://github.com/mbianchidev/stainless/issues/24) |
| 8 | Validated additional desktop platforms, secure updates, and stable/public-release readiness | [#6](https://github.com/mbianchidev/stainless/issues/6), [#25](https://github.com/mbianchidev/stainless/issues/25), [#26](https://github.com/mbianchidev/stainless/issues/26), [#27](https://github.com/mbianchidev/stainless/issues/27) |

Security, documentation, Chromium maintenance, and the packaging path in #37 apply to **every**
phase. #4, #6, #25, #26, and #27 have early slices even where their full scope lands later.
An issue stays open until its full acceptance criteria pass; phase completion is determined by the
epic's explicit release checklist, not by prematurely closing a cross-cutting issue.

### Phase 1: Vanilla Chromium on macOS

**Outcome:** An authorized user can download a DMG, install the app, browse, and receive automated
unsigned updates without compiling it. This is the minimum useful release, not a tooling-only milestone.

1. **Freeze the baseline contract (#2, baseline review from #4).** Start with Apple Silicon macOS
   and upstream Chromium behavior. Record process/storage boundaries, background service traffic,
   unsupported Google-service integrations, and the privacy features not yet implemented.
2. **Pin and build the real browser (#3).** Keep Chromium history outside the Stainless repository,
   record the exact revision and minimal patch stack, and build Chromium's full `chrome` target.
   `content_shell`, an Electron demo, and system-WebView wrappers do not meet this requirement.
3. **Make local source builds repeatable (#5).** Pin the relevant toolchains and GN arguments;
   document build-host macOS, Xcode/SDK, APFS, disk/RAM requirements, fetches, dependency restoration,
   compilation, tests, and packaging through a local entry point. Add WebUI/model/server dependencies
   only when used. Record verified versions during implementation, not invented setup commands now.
4. **Preserve everyday Chromium browsing (#36).** Include navigation and normal provider search,
   tabs/windows, bookmarks/history, find/zoom, downloads, print/PDF, settings, certificate warnings,
   Incognito, and built-in DevTools. Default skipped search selection to DuckDuckGo; sanitization
   arrives in Phase 4. Use distinct Stainless bundle, user-data/cache, and keychain identities.
   Never implicitly reuse Chrome/Chromium data or require Google API credentials.
5. **Bundle the app and DMG (#37).** Produce a relocatable non-component release `.app` with all
   frameworks, helpers, resources, locales, and notices, then a versioned Apple Silicon DMG.
   Neither source-tree paths nor developer dependencies may be needed at runtime.
6. **Add automated unsigned updates (#6's Phase 1 slice).** Check the configured preview channel
   on startup and periodically, automatically download/stage the matching newer build, verify its
   hash, and install it on the next user-controlled restart. Include update status, retry/error
   reporting, profile-safe replacement, and failed-launch recovery. Meet the contract below;
   requiring a new manual DMG download does not count.
7. **Exercise the downloaded package (#37, baseline checks from #26).** On a separate clean user
   or supported Mac without the checkout/toolchain, download it, verify the hash, install, eject the
   image, and launch from Applications. Publish a newer preview and exercise the real automatic
   N-to-N+1 download, installation/relaunch, and state preservation; also test failure recovery and
   removal. Check sandboxing, TLS errors, Incognito history isolation, and downloads with synthetic fixtures.
8. **Publish private prereleases (#37, minimum docs from #27).** Attach the actual DMG, updater
   payload/feed metadata, SHA-256 checksums, source/Chromium/toolchain/build manifests, required
   notices, install/removal steps, update/recovery instructions, and known limitations to accessible
   GitHub releases. A local path, screenshot, successful compile, or expiring CI artifact is not a release.

**Exit gate:** The installed browser works after the DMG is ejected on a supported Mac with no
developer tools. All baseline browsing flows work; Chrome data is untouched; telemetry is disabled;
sandboxing and certificate validation stay enabled. The installed N build automatically discovers,
downloads, and installs an unsigned N+1 build on a controlled restart without a manual reinstall,
preserving supported profile/session state and retaining a usable installation on failure.

**Not prerequisites:** Full profile encryption, the custom vault, advanced shell features, Tor,
agents, AI, extensions, signed update metadata/transparency infrastructure, Linux/Windows, Apple
signing/notarization, a docs site, or hosted CI. Automatic unsigned updates are a prerequisite.
This preview is not a claim of complete v1 privacy or stable readiness.

Implementation references:
[Chromium macOS builds](https://chromium.googlesource.com/chromium/src/+/main/docs/mac_build_instructions.md),
[Apple Silicon builds](https://chromium.googlesource.com/chromium/src/+/main/docs/mac_arm64.md), and
[Apple's per-app launch exception](https://support.apple.com/en-us/102445). These upstream commands
are references, not an already implemented Stainless bootstrap.

#### Phase 1 automatic update contract

#6 owns the updater; #37 publishes and tests its assets. The initial implementation must:

- Check/download automatically once the preview channel and any private-download authorization
  are configured. Keep a visible manual check, available/staged/error status, and restart control.
  Update checks must not force-close active work or make browsing depend on network availability.
- Fetch version/channel/OS/architecture/asset/hash metadata and the payload from explicitly trusted
  HTTPS release sources with normal certificate validation. Reject wrong-target, stale/downgrade,
  malformed, incomplete, or hash-mismatched updates without replacing the installed app.
- Support the private repository's real download authorization without embedding publisher tokens
  in the app, feed, or logs. Store any user-provisioned scoped credentials in the OS keychain;
  ordinary browsing must not require a GitHub/Stainless account.
- Stage a complete relocatable app, handle installation permissions explicitly, replace only at a
  safe quit/restart boundary, and keep the last known-good app plus profile backups. Interrupted
  downloads or replacement/launch failures must not destroy the usable installation or silently
  downgrade an incompatible profile schema.
- Demonstrate installed N-to-N+1 updating on a supported clean user/host, preserving bookmarks,
  normal session state, and sandboxing. Exercise offline/access-denied responses, bad hashes,
  wrong versions/targets, interrupted download/replacement, and failed-launch recovery locally.
- State that unsigned payloads/feed metadata rely on HTTPS and release-host/account trust.
  SHA-256 detects corruption, not an independently authenticated publisher, a compromised release
  host, or replay-resistant signed metadata. Production signing/transparency remains later work.

Manual download-and-replace remains documented recovery only. No signing keys, notarization,
CI workflow, telemetry, source checkout, or developer tools are prerequisites for this update path.

### Phase 2: Everyday browser and identities

**Outcome:** The Phase 1 browser gains useful organization, separate profiles, and recovery.

1. Implement process/key/storage boundaries and versioned encrypted-store migrations in #4.
   Protect the existing preview profile with a backup and explicit migration/restore rules;
   do not silently reuse plaintext data under an encryption claim.
   The first bounded [profile ownership/migration slice](profile-isolation.md) is opt-in
   developer groundwork, not completion of encrypted-store or protected-data isolation.
2. Add profile/space primitives and the policy matrix in #7. Ship Standard, Incognito, and Guest
   first. Keep future Privacy Container, Developer, Agent Sandbox, and Tor modes unavailable
   until their capability owners implement and verify them.
   The current opt-in tool supplies the first Standard/fresh-root Incognito policies and
   foreground consumer. Guest remains unavailable pending memory-only storage, restricted
   downloads and guaranteed teardown; spaces, immutable shell tabs and stronger boundaries
   remain open. This source slice does not publish or verify a new browser preview.
3. Add the shell improvements in #8: keyboard commands, optional vertical/tree tabs, groups,
   splits, spaces, lazy restoration, dashboard, consented migration, skippable onboarding, and
   startup safe mode. Show only available capabilities; vault/extension/sync imports and their
   dashboard/onboarding integrations land with those capabilities, not before.
4. Add local tab/process attribution and safe manual resource controls in #29. Do not silently
   lose unsaved state or impose ordinary-browsing hard limits by default. Later extension,
   agent, and model owners add their metrics when those runtimes exist.

**Exit gate:** Separate profiles cannot read each other's protected data; migration/restart/safe-mode
paths preserve user data; normal browsing remains usable with productivity UI hidden. Resource
controls accurately label shared/approximate usage. Repackage and install the new DMG.

#33 is bounded Zen UX research feeding #8, not permission to change the engine, copy assets, expand
scope automatically, or delay Phase 1.

### Phase 3: Privacy, audit, and vault

**Outcome:** The existing browser gains enforceable, explainable privacy and credential controls.

1. Build the encrypted audit/event/redaction foundation in #10 on #4. It does not depend on
   completion of every privacy feature in #9; privacy adapters can follow the core store.
2. Build #9 on the profile/mode and audit foundations: blocking, permission ledger, firewall,
   proxy/DNS controls, explicit boundary transfers, quarantine, panic, and expiring exceptions.
   Add passive TLS/certificate visibility in #28 without weakening Chromium validation.
3. Implement the vault in #11, including origin-bound autofill, locking/recovery, revocable secret
   handles, and explicitly approved imports. Ordinary nonsecret browsing stays usable while locked.
4. Add Privacy Containers and evaluate #30's coherent per-site identities against a documented
   baseline and threat model. Publish empirical limits; tests cannot prove universal unlinkability
   or anonymity. Do not enable a fingerprint change that lacks convincing privacy/compatibility
   evidence. Agent integrations remain gated until Phase 6.

**Exit gate:** Synthetic policy/isolation tests exercise denial and fail-closed paths, block reasons
are inspectable, vault locking invalidates handles, and exported evidence is redacted. Keep the
ordinary browser usable when these optional tools are disabled and ship the next DMG.

### Phase 4: Clean search, sync, and Tor

**Outcome:** Users can clean search results, pair their own devices, or explicitly choose Tor routing.

1. Ship #13's direct-to-provider clean view, original-view escape hatch, removal report, and
   visible failure behavior. Provider terms and supported transforms are documented.
2. Ship optional E2EE sync/backups/restore in #12 after the vault and audit foundations. Validate
   two-device pairing, conflict handling, revocation, offline use, and recovery without a
   server-side reset. Later workspace/extension data integrates only when implemented.
3. Implement #14 after the mode, network, audit, and clean-search foundations. Clearly separate
   convenient Tor Proxy from experimental Hardened Onion; verify route/DNS/WebRTC/storage
   isolation, daemon/bridge failure, and no clearnet fallback before enabling a mode.
4. Gate Hardened Onion's availability and claims on its specific review and tests. Chromium over
   Tor is not equivalent to Tor Browser's anonymity set; do not make that promise or reuse #30's
   randomized identities. Keep AI, extensions, vault, agents, and local/repository access prohibited.

**Exit gate:** Search failures never masquerade as authoritative results, sync never requires a
Stainless account or exposes plaintext to its server, and Tor failure cannot leak to clearnet.
The packaged browser works with sync and both Tor modes disabled.

### Phase 5: Extensions and workspaces

**Outcome:** The browser can safely run supported extensions and trusted repository tools.

1. Add #15's MV3 compatibility, permission/update review, private-mode isolation, and metrics
   from #29. Validate actual Chrome Web Store availability/terms and extension compatibility;
   do not promise Chrome sign-in/sync or proprietary services that a Chromium fork cannot supply.
2. Add #16's Rust/WASM capability runtime, SDK, verifiable packages, and shared registry
   infrastructure. App/DMG signing deferral does not waive extension/package verification.
3. Add #17's workspace trust/configuration and scoped policies. Repository configuration never
   executes on clone/open before trust; later skill/agent references remain inert.
4. Add #18's approved Git operations, small editor, audited terminal, and isolated tool execution.
   Keep Porto optional and read-only; Stainless does not acquire service lifecycle controls.

**Exit gate:** Capability escalation stops for approval, untrusted repositories cannot execute tools,
extensions cannot bypass mode restrictions, and workspace writes/remote actions remain approved
and auditable. Ship a new DMG without making extensions, repositories, or Porto mandatory.

### Phase 6: DevTools and agents

**Outcome:** Users can debug and replay a session, then run bounded automation without risking their
primary browsing identity.

1. Add #19's replay/network lab and #20's API/testing/WASM tools. Keep development CAs and TLS
   interception restricted to Developer and Agent Sandbox modes; mark nondeterministic evidence.
2. Add #21's disposable sandboxes, mandatory resource ceilings, policy evaluation, audited
   multi-agent bus, reviewed artifact promotion, pause/kill/takeover, and prompt-injection tests.
   No agent gets ambient primary-profile credentials or arbitrary host writes.
3. Add #22's policy-enforcing desktop/headless interfaces and authenticated MCP transport.
   Pin and test actual Playwright/CDP/ChromeDriver/BiDi compatibility, including policy-bypass
   attempts through lower-level interfaces. Scheduling stays external.
4. Add #31's signed skill lifecycle on #16's registry and #21's enforcement. A signature never
   grants capabilities by itself; revocation and capability-expanding updates fail closed.

**Exit gate:** A synthetic site can be captured, inspected/replayed, turned into a reviewed test,
and exercised by an agent that cannot exceed its budget or export unapproved state. Desktop and
headless share policy conformance. Package the browser and the separately usable headless artifact.

### Phase 7: Local AI and research

**Outcome:** Optional AI and offline knowledge tools enhance, but never gate, the working browser.

1. Add #23's provider-neutral adapters, verified model downloads, context previews, and local
   resource controls. Validate actual model availability, licenses, hardware fit, and pinned
   backend versions before selecting defaults; old model names are not evidence of compatibility.
2. Add explicit context consent and previewed actions, local-first translation/media tools, and
   opt-in retained memory. Hardened Onion prohibits AI; private/Tor Proxy cloud use needs separate
   per-session consent.
3. Add #24's encrypted notes, reader/RSS, captures, citations, and profile-scoped local search.
   Default capture viewing is non-networked and script-disabled; replay requires a separate sandbox.
4. Integrate user-invoked search summaries and reproducibility manifests without automatic
   background provider requests.

**Exit gate:** All earlier browsing flows still work with AI disabled and models absent. No context
is sent without approval; captures verify their hashes and export usefully without Stainless.
Ship another complete browser package.

### Phase 8: Cross-platform and stable readiness

**Outcome:** The feature-complete browser gains validated additional desktop targets and stable
release operations, without retroactively blocking the earlier usable previews.

1. Harden #6's existing automatic updater with signed metadata/artifacts, offline verification,
   transparency, stronger rollback protection, and supply-chain evidence.
   Developer ID signing/notarization and production signing-key operations are later work, not
   requirements of the current planning task or unsigned previews.
2. Extend #25 from the macOS preview path to Linux/Windows packages and richer OS integration.
   State the actual tested OS/architecture matrix; Intel/universal macOS is not assumed.
3. Complete #26's full hardening, compatibility, independent-review, and stable-API gates.
   Resolve blocking findings before advertising stable security/anonymity guarantees.
4. Complete #27's versioned public schemas, offline/reference docs, contributor guidance, and
   release recovery instructions. Choose license/disclosure policy before public publication,
   without inventing those decisions in this private planning change.

**Exit gate:** Every mandatory v1 capability and stable gate passes on every advertised platform.
Publish the appropriate packages and evidence; do not claim support for untested systems.

## Release gate for every phase

- [ ] Pin the product/Chromium revisions, dependencies, toolchains, and build configuration; produce
  a repeatable local source build. Bit-for-bit artifact reproducibility is separately tracked in #6.
- [ ] Keep Chromium security maintenance active from Phase 1. Record each baseline and review known
  findings before release; target critical fixes within 24 hours and other security fixes within
  72 hours as in #6. These are engineering targets, not a claim of an existing service.
- [ ] Pass the baseline browser smoke flows and the tests for every newly enabled capability.
  Keep Chromium sandboxing, site isolation, and certificate checks enabled; no automatic telemetry.
- [ ] Download and install the actual package on a supported clean user/host, eject its image,
  then browse, quit/restart, upgrade, and uninstall without a checkout or development tools.
- [ ] Publish a versioned downloadable package, final-asset hashes, build/source manifests, notices,
  supported-system requirements, install/removal/upgrade instructions, and known limitations.
- [ ] From Phase 1, exercise automatic checks/downloads and unsigned N-to-N+1 installation on a
  user-controlled restart, preserving profile/session state. Test failed-download/install recovery;
  keep manual replacement as a fallback only and stop promoting superseded vulnerable previews.
- [ ] Preserve the last usable package and documented profile backups/migrations. Rollback must
  respect data-schema compatibility and security downgrade boundaries, not just replace a binary.
- [ ] Treat exploitable critical/high findings as blockers absent explicit, documented, expiring
  risk acceptance. Hashes alone are not authenticated update metadata.
- [ ] Gate incomplete features and privacy claims. Maintain user control, optional AI/cloud/sync,
  redacted diagnostics, keyboard/accessibility behavior, and previous working browsing flows.

These are release criteria for implementation. They are not checked off by tooling/unit fixtures,
and the broad stable/agent/Tor review suite is not a Phase 1 prerequisite.

## Ownership boundaries

Every capability has one primary owner; later integrations can add explicit scoped dependencies.

- #36 owns the initial upstream browsing baseline; #8 owns the enhanced Stainless shell.
- #37 owns the unsigned Apple Silicon package/download/install path reused across phases;
  #25 owns additional platforms/native integrations. #6 owns automated unsigned updates from
  Phase 1, then production signing and stronger verification in Phase 8. Packaging depends only
  on its Phase 1 updater slice, not the full signing/transparency issue.
- #4 supplies a Phase 1 baseline review, then stronger storage/process boundaries in Phase 2.
  #26 owns per-phase security checks and the later full stable-review gate.
- #10 owns the audit schema/storage/redaction foundation; privacy adapters integrate #9 afterward.
  Full privacy implementation is not a prerequisite for creating the audit foundation.
- #19 owns workspace development CAs and active TLS interception; #28 owns passive certificate,
  Certificate Transparency, pinning, and downgrade controls.
- #8 owns dashboard/hibernation UX; #29 owns resource attribution and manual controls. Extension,
  agent, and model owners supply adapters when available, without blocking the base resource view.
- #9 owns entropy reporting/privacy receipts; #30 owns randomized container identities;
  #14 keeps Hardened Onion standardized fingerprints and owns the limits of anonymity claims.
- #21 owns runtime policy/ceilings; #31 owns skill packaging/lifecycle, reusing #16's registry.
- #27 owns documentation readiness throughout; future public schemas do not block Phase 1
  installation, source instructions, third-party notices, or private download access.

## Resolved conflicts and ambiguities

- **Release policy:** Usable, downloadable previews begin in Phase 1. The historical prohibition
  on public/private dogfood binaries before complete v1 is superseded, not an additional gate.
- **Signing:** Unsigned/unnotarized macOS previews are acceptable. Apple distribution signing is
  deferred; integrity checks and signature requirements for skills/extensions are not waived.
- **Updates:** Automated unsigned updates are required in Phase 1. Manual upgrades are recovery
  only; signed update metadata, publisher authentication, and transparency are later hardening.
- **CI:** The original planning task changed Markdown/issues only. Implementation uses local checks;
  neither live nor dormant CI is a prerequisite for the first useful package.
- **Built-in modes:** Seven eventual tab modes (Standard, Incognito, Privacy Container, Developer,
  Agent Sandbox, Tor Proxy, Hardened Onion), plus Guest as a separate memory-only profile.
- **Tor:** Hardened Onion bundles pinned Tor; Tor Proxy may use bundled or user-managed Tor.
  Neither routing alone nor a leak test establishes Tor Browser-equivalent anonymity.
- **Leak testing:** Configuration-time DNS/route checks are user-facing; comprehensive isolation
  and fingerprint tests are developer-run initially and may be automated later.
- **AI:** Hardened Onion prohibits AI. Private/Tor Proxy cloud AI needs explicit per-session opt-in.
  Arbitrary inert local weights are allowed; executable custom code needs approval and isolation.
- **Collaboration:** Live multi-user browsing/debugging is excluded; same-user observation and
  takeover of an agent session remains in scope.
- **CAPTCHAs:** Strategies, solvers, stealth changes, payments, and handoff remain policy-gated.
- **Resource limits:** Ordinary-browsing hard limits default off; agent ceilings are mandatory.
- **Accessibility/performance:** Preserve Chromium accessibility and avoid Stainless UI regressions;
  no numerical performance or reliability threshold has been approved as a v1 release gate.
- **Repository analysis:** Runtime JS/WASM supply-chain inspection is in scope; repository
  manifest/lockfile dependency analysis is excluded.
- **License/disclosure:** Neither is chosen yet. Required third-party redistribution notices apply
  even to private previews; maintainer license/disclosure decisions precede public publication.
- **Telemetry-free rollout:** Later health pauses use publisher/manual signals or separately
  consented minimal updater health data, not browsing telemetry.
- **User agent:** Stainless eventually identifies distinctly, with visible expiring per-site
  compatibility overrides; do not couple basic Phase 1 browsing to later fingerprint machinery.

## Working conventions

- Use `area:*` labels for ownership and `Phase N - <theme>` milestones for primary delivery.
- Link every implementation issue under #1. Keep #33 explicitly research-only.
- State prerequisites for the current slice separately from later integration dependencies;
  cross-phase parent issues remain open until their full criteria pass.
- Keep each implementation slice tied to its approved delivery phase and real release evidence;
  the original planning-only restriction does not prohibit the subsequent Phase 1 implementation.
- Record each phase's release/version, evidence, limitations, and remaining gates in the epic
  before calling that phase complete.
