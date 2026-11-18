# Project status

**Unsigned preview candidates are downloadable. Phase 1 verification is still in progress.**

This page summarizes what is published, what exists only in source and which release gates
remain open. [Releases](https://github.com/mbianchidev/stainless/releases) are the source of
truth for published versions; the linked docs hold the detailed evidence.

## Published previews

- The newest published download is **0.1.0-preview.4**, an unsigned, unnotarized Apple
  Silicon prerelease built on Chromium **156.0.8078.80**. It is older than the current source
  pin below, so it lacks later Chromium security fixes until a newer preview is published.
- Preview.4 is the de-googled five-patch baseline: Stainless identity, search and telemetry
  defaults, macOS updater hooks, branding and Google-service removal.
- **Google-backed threat/reputation and password-leak checks have no replacement yet.**
  Read [removed services, compatibility and replacement gaps](degoogling.md) before using it.
- The published baseline built and passed isolated browser smoke checks locally. The actual
  DMG installs and browses after eject in an isolated local test directory.

Install, update and removal steps are in the [macOS install guide](install-macos.md).
A Homebrew cask is planned but not available yet; install from Releases.

## Current source

Current source targets **0.1.0-preview.8, build 8**, pinned to official Mac Stable Chromium
**158.0.8127.37** ([`chromium/manifest.json`](../chromium/manifest.json)). It is not
published: no preview.8 installer exists, and installed previews do not gain these features.

Beyond the published baseline, current source adds:

- Removed service settings and managed Standard/Incognito modes, with a native profile
  chooser, cold backup and clone-restore ([profile isolation](profile-isolation.md)).
- Explicit managed [startup Safe Mode](profile-isolation.md#managed-startup-safe-mode).
- A local, keyboard-first [browser command palette](build-macos.md#browser-command-palette),
  a native [profile shortcut editor](build-macos.md#profile-keyboard-shortcuts) and
  [portable keymaps](keymaps.md).
- A read-only [resource overview](resource-overview.md),
  [local-only saved history](history-privacy.md) and
  [Stainless internal addresses](internal-urls.md).
- Experimental, opt-in native [Web Push](web-push-self-hosting.md) and
  [local services](local-services.md).

Local **preview.5** to **preview.7** candidates were built and checked locally but never
published. The preserved Chromium 158.0.8127.18 preview.5 candidate remains experimental;
the current 158.0.8127.37 source has separate verified provenance. See
[Maintaining Chromium](build-macos.md#maintaining-chromium) and the retained local evidence
in the [install guide](install-macos.md#local-development-candidate).

## Open release gates

Separate clean-Mac verification and the authorized automatic N-to-N+1 update remain open.
Local checks are not a completed phase. Track the actual gates in
[the Phase 1 contract](phase-one.md#release-evidence) and
[epic #1](https://github.com/mbianchidev/stainless/issues/1), and record installed-update
evidence with the [clean-user procedure](update-verification.md).

## Scope

Phase 1 retains ordinary Chromium browsing. Encrypted profiles, custom privacy modes, Tor,
agents, enhanced DevTools, AI and clean-search transforms are later features, not current
privacy guarantees. Linux and Windows are later targets; mobile is outside v1. See the
[roadmap](roadmap.md) for delivery phases and non-goals.

Remaining open-source readiness work is tracked in
[#27](https://github.com/mbianchidev/stainless/issues/27).
