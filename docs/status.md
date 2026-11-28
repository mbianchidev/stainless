# Project status

**Unsigned preview candidates are downloadable. Phase 1 verification is still in progress.**

This page summarizes what is published, what exists only in source and which release gates
remain open. [Releases](https://github.com/stainless-browser/stainless/releases) are the source of
truth for published versions; the linked docs hold the detailed evidence.

## Published previews

- The newest published download is **0.1.0-preview.9, build 9**, an unsigned, unnotarized
  Apple Silicon prerelease built on official Mac Stable Chromium **159.0.8154.34**
  ([release](https://github.com/stainless-browser/stainless/releases/tag/stainless-preview-v0.1.0-preview.9),
  2026-11-25, source `b6fdeea`). Release CI re-verified its checksums, source tag,
  license/security policy, current Chromium Stable and dependency advisories before publishing.
  Beyond preview.8 it adds on-device
  [threat-list blocking and opt-in password breach checks](threat-protection.md), token-free
  updates from public releases, a [memory-only managed Guest](profile-isolation.md#memory-only-guest),
  a one-color S in the menu bar and llama.cpp b12603 for CVE-2026-133710. Its updater lists
  releases by the repository's numeric GitHub ID, so it follows the move to
  `stainless-browser/stainless`.
- **0.1.0-preview.8** (Chromium 158.0.8127.37) remains published. Its updater can't follow
  the repository move; install preview.9 or later by hand.
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
- **Preview.8 and earlier have no replacement for Google-backed threat/reputation and
  password-leak checks**, and no preview replaces download reputation. Read
  [removed services, compatibility and replacement gaps](degoogling.md) before using them.
- Preview.9 passed built-app browser smoke checks in an isolated macOS VM. Its published
  `release-status.json` marks clean-host install, installed N-to-N+1 update and installed
  failure recovery as not recorded.
- **0.1.0-preview.4** (Chromium 156.0.8078.80) also remains published.
  It lacks later Chromium security fixes; use it only as the N build of an update test.
  Local preview.5 to preview.7 candidates were never published.

Install, update and removal steps are in the [macOS install guide](install-macos.md).
A Homebrew cask is planned but not available yet; install from Releases.

## Current source

The default branch is versioned **0.1.0-preview.10, build 10**
([`chromium/manifest.json`](../chromium/manifest.json)). The repository moved from
`mbianchidev/stainless` to the `stainless-browser` organization; current source publishes
there and points model downloads and links at the new owner. Developers can opt in to a
loopback [compile cache](compile-cache.md). The About and `chrome://version` pages show the
Stainless version and build first, then the Chromium version it is built on, and the
[resource overview](resource-overview.md#manual-controls) can hibernate, wake or end this
window's tabs and renderers when no unsaved state is at risk. These do not change installed
previews. Current source also pins llama.cpp v1.1.0, Tesseract 5.5.5 and a fixed Leptonica
commit for CVE-2026-137057, CVE-2026-137567, nine Tesseract advisories and CVE-2026-137063
([local services](local-services.md#image-description-feasibility)).
The fifteen optional [local model packs](local-services.md#published-model-packs) are published
as an opt-in prerelease. Speech, PDF OCR and [Web Push](web-push-self-hosting.md#browser-lifecycle-coverage)
lifecycle coverage now passes in an isolated VM.

Current source also fixes the New Tab page's "Aw, Snap! Error code: 6" crash in preview.9.
Builds no longer turn DCHECKs into crashes or apply Chromium's bot-only field-trial testing
config ([release arguments](build-macos.md#sources-caches-and-pins)). It adds
[automatic Safe Mode](profile-isolation.md#automatic-safe-mode-after-repeated-failed-startups)
after two failed startups, optional account-free [first-run setup](first-run-setup.md), a
distinct Stainless user agent with expiring per-site
[Google Chrome identity overrides](site-compatibility.md), and a versioned
[architecture and threat model](architecture-threat-model.md). Safe Mode can
[review the previous session](session-restore.md#review-the-previous-session-in-safe-mode) and
reopen chosen tabs, restored tabs can [load only when selected](session-restore.md), the
resource overview can [pause tabs](resource-overview.md#pause-and-resume) and enforce an opt-in
[background tab memory limit](resource-overview.md#background-tab-memory-limit), and
[settings transfer](settings-transfer.md) exports and imports an allowlist of settings after a
preview. Each new profile root gets its own
[Safe Storage Keychain item](profile-isolation.md#per-root-safe-storage-keys), and browser tests
enforce the [managed mode policy matrix](profile-isolation.md#managed-browsing-modes).
A newer preview needs a fresh version and the [release process](releasing.md).

## Open release gates

Separate clean-account verification and an actual automatic N-to-N+1 update remain open
([#46](https://github.com/stainless-browser/stainless/issues/46)). Preview.9 and later
updaters follow the repository move, so preview.9 is the earliest eligible N build: install it
in a separate macOS account, then let its updater discover, download and install a later
preview. Preview.4 and preview.8 updaters can't reach releases after the move. Follow the
[clean-user procedure](update-verification.md) and record results in
[epic #1](https://github.com/stainless-browser/stainless/issues/1). Local checks are not a
completed phase; see [the Phase 1 contract](phase-one.md#release-evidence).

## Scope

Phase 1 retains ordinary Chromium browsing. Encrypted profiles, custom privacy modes, Tor,
agents, enhanced DevTools, AI and clean-search transforms are later features, not current
privacy guarantees. Linux and Windows are later targets; mobile is outside v1. See the
[roadmap](roadmap.md) for delivery phases and non-goals.

Remaining open-source readiness work is tracked in
[#27](https://github.com/stainless-browser/stainless/issues/27).
