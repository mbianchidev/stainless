# Stainless

<img src="chromium/branding/stainless-icon.png" width="96" height="96" alt="Stainless icon">

A work in progress privacy-first browser.

Stainless is a free, Chromium-based browser project for developers, agent builders and
privacy-conscious users. 

It will be (eventually, maybe) donation/sponsorship funded

Aims to let users navigate the web without ads, sponsored placements, behavioral tracking or a mandatory account.

## Status (updated via agent)

**Unsigned preview candidates are downloadable. Phase 1 verification is still in progress.**

The repository includes pinned Chromium/depot_tools sources, a focused downstream patch stack,
a Rust build/release coordinator, a native macOS updater and synthetic tests. Chromium history
is fetched outside this checkout.

**The published preview baseline built and passed isolated browser smoke checks locally.**
The actual DMG installs and browses after eject in an isolated local test directory.
Download previews from the [releases](https://github.com/mbianchidev/stainless/releases).
Separate clean-Mac verification and the authorized automatic N-to-N+1 update remain open.
Local checks are not a completed phase. Track actual release gates in
[the Phase 1 contract](docs/phase-one.md#release-evidence) and
[epic #1](https://github.com/mbianchidev/stainless/issues/1).

This checkout targets the de-googled preview baseline. **Google-backed threat/reputation and
password-leak checks have no replacement yet.** See
[removed services, compatibility and replacement gaps](docs/degoogling.md) before using it.
Local **preview.5** builds include the removed service settings and native profile chooser/recovery.
The published download is still preview.4. The preserved Chromium 158 preview.5 candidate remains
experimental/unpublished; the current .37 Stable source has separate verified provenance.
Current source also adds explicit managed startup Safe Mode and a local, keyboard-first
[browser command palette](docs/build-macos.md#browser-command-palette), with a native
[profile shortcut editor](docs/build-macos.md#profile-keyboard-shortcuts). These are not
installed-preview updates. See the [build guide](docs/build-macos.md#maintaining-chromium).

## Build locally

Start on Apple Silicon macOS with full Xcode 26.6+, macOS SDK 26.5+, APFS and substantial free disk.
The entry point verifies or locally installs pinned Rust 1.99.1. Xcode is required on the build
Mac only, not for installed browsing; no paid Apple account or signing credential is needed.

```sh
git clone git@github.com:mbianchidev/stainless.git
cd stainless
./tools/stainless doctor
./tools/stainless bootstrap
./tools/stainless build
./tools/stainless test
./tools/stainless package
```

See [build prerequisites, commands, caches and publication](docs/build-macos.md) before fetching
Chromium. Preview packages are unsigned/unnotarized; installation/update/recovery guidance is
in [the macOS guide](docs/install-macos.md).

## Documentation and scope

- [Phase 1 baseline, boundaries and outstanding release gates](docs/phase-one.md).
- [Clean-user automatic-update verification](docs/update-verification.md).
- [Managed Standard/Incognito, startup Safe Mode and recovery](docs/profile-isolation.md).
- [Local read-only tab/process resource overview (current source)](docs/resource-overview.md).
- [Saved-history privacy and extension compatibility (current source)](docs/history-privacy.md).
- [Stainless internal addresses and canonical compatibility (current source)](docs/internal-urls.md).
- [Local-service feasibility and build-only model probes (not restored features)](docs/local-services.md).
- [Self-hosted Web Push fixture and protocol evidence (not installed-browser proof)](docs/web-push-self-hosting.md).
- [Delivery phases, v1 features and non-goals](docs/roadmap.md).
- [Historical decisions Q&A](docs/decisions-q-and-a.md).
- [Execution epic and capability issues](https://github.com/mbianchidev/stainless/issues/1).

Phase 1 retains ordinary Chromium browsing. Encrypted profiles, custom privacy modes, Tor,
agents, enhanced DevTools, AI and clean-search transforms are later features, not current
privacy guarantees. Linux/Windows are later targets; mobile is outside v1.

## Contributing and license

Start with the epic and owning issue; follow the [working conventions](docs/roadmap.md#working-conventions).
For engine/build/release work, follow [Chromium maintenance](docs/build-macos.md#maintaining-chromium).
The repository is public. [CI](docs/build-macos.md#continuous-integration) checks source/native
changes without replacing installed-browser release evidence. Remaining open-source readiness
work is tracked in [#27](https://github.com/mbianchidev/stainless/issues/27).
Report vulnerabilities privately as described in the [security policy](SECURITY.md).

Stainless copyright is held by **Synctune Open**. Stainless source is licensed under the
[Apache License 2.0](LICENSE); contributions are accepted under the same license.
Chromium and other bundled components keep their own licenses; their notices ship with each release.
