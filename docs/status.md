# Project status

**Unsigned preview candidates are downloadable. Phase 1 verification is still in progress.**

This page summarizes what is published, what exists only in source and which release gates
remain open. [Releases](https://github.com/mbianchidev/stainless/releases) are the source of
truth for published versions; the linked docs hold the detailed evidence.

## Published previews

- The newest published download is **0.1.0-preview.8, build 8**, an unsigned, unnotarized
  Apple Silicon prerelease built on official Mac Stable Chromium **158.0.8127.37**
  ([release](https://github.com/mbianchidev/stainless/releases/tag/stainless-preview-v0.1.0-preview.8),
  2026-11-18, source `15d3332`). Release CI re-verified its checksums, source tag,
  license/security policy, current Chromium Stable and dependency advisories before publishing.
- Beyond the de-googled preview.4 baseline, preview.8 adds:
  - Removed service settings and managed Standard/Incognito modes, with a native profile
    chooser, cold backup and clone-restore ([profile isolation](profile-isolation.md)) and
    explicit managed [startup Safe Mode](profile-isolation.md#managed-startup-safe-mode).
  - A local, keyboard-first [browser command palette](build-macos.md#browser-command-palette),
    a native [profile shortcut editor](build-macos.md#profile-keyboard-shortcuts) and
    [portable keymaps](keymaps.md).
  - A read-only [resource overview](resource-overview.md),
    [local-only saved history](history-privacy.md) and
    [Stainless internal addresses](internal-urls.md).
  - Experimental, opt-in native [Web Push](web-push-self-hosting.md) and
    [local services](local-services.md).
- **Published previews have no replacement for Google-backed threat/reputation and
  password-leak checks.** Read [removed services, compatibility and replacement gaps](degoogling.md)
  before using them.
- Preview.8 passed built-app browser smoke checks on the build Mac. Its published
  `release-status.json` marks clean-host install, installed N-to-N+1 update and installed
  failure recovery as not recorded.
- **0.1.0-preview.4** (Chromium 156.0.8078.80) remains published as the previous preview.
  It lacks later Chromium security fixes; use it only as the N build of an update test.
  Local preview.5 to preview.7 candidates were never published.

Install, update and removal steps are in the [macOS install guide](install-macos.md).
A Homebrew cask is planned but not available yet; install from Releases.

## Current source

The default branch is still versioned **0.1.0-preview.8, build 8**
([`chromium/manifest.json`](../chromium/manifest.json)). Since the preview.8 tag, current
source moves to official Mac Stable Chromium **159.0.8154.13**, adds on-device
[threat-list blocking and opt-in password breach checks](threat-protection.md), lets the
updater read public releases without a GitHub token, adds a
[memory-only managed Guest](profile-isolation.md#memory-only-guest) and shows a one-color S
in the menu bar. Local services use llama.cpp b12603 for CVE-2026-133710. Developers can
opt in to a loopback [compile cache](compile-cache.md). These do not change installed previews. A newer preview needs a fresh version and the
[release process](releasing.md).

## Open release gates

Separate clean-account verification and an actual automatic N-to-N+1 update remain open
([#46](https://github.com/mbianchidev/stainless/issues/46)). Preview.4 and preview.8 are both
public and form an eligible pair: install preview.4 in a separate macOS account, then let its
updater discover, download and install preview.8. Startup discovery and managed-root coverage
need a follow-up run from preview.8 to a later preview. Preview.4 and preview.8 updaters still
need a read-only GitHub token even though the repository is public. Follow the
[clean-user procedure](update-verification.md) and record results in
[epic #1](https://github.com/mbianchidev/stainless/issues/1). Local checks are not a
completed phase; see [the Phase 1 contract](phase-one.md#release-evidence).

## Scope

Phase 1 retains ordinary Chromium browsing. Encrypted profiles, custom privacy modes, Tor,
agents, enhanced DevTools, AI and clean-search transforms are later features, not current
privacy guarantees. Linux and Windows are later targets; mobile is outside v1. See the
[roadmap](roadmap.md) for delivery phases and non-goals.

Remaining open-source readiness work is tracked in
[#27](https://github.com/mbianchidev/stainless/issues/27).
