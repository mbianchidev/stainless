# Building on Apple Silicon macOS

The local coordinator builds Chromium's full `chrome` target. It does not download and
rebrand an existing browser, use Electron, or substitute a system WebView.

**Status:** the published preview baseline built and passed isolated browser smoke checks on
Apple Silicon macOS 26.6 with Xcode/SDK 27.2 and the Metal Toolchain 27B47.
The unsigned DMG/ZIP/feed are published as private preview candidates. DMG install/eject
and packaged browsing pass locally; clean-Mac and automatic-update gates remain open.
Full Xcode is a build prerequisite, not an end-user dependency.
The sixth-patch mode/settings changes and native managed-profile chooser/recovery now build
locally in **preview.5**. Actual isolated-browser smoke covers the removed settings and managed
Incognito; native chooser/recovery checks remain offscreen synthetic evidence, not installed
native-workflow verification. This explicitly requested Chromium 158 candidate is experimental,
not a published preview or a passing Mac Stable provenance check for that preserved
158.0.8127.18 candidate. The current source pin's separate evidence is recorded under
[Maintaining Chromium](#maintaining-chromium).

## Requirements

| Requirement | Baseline |
| --- | --- |
| Build host | Apple Silicon Mac with an APFS source/cache volume |
| Xcode | Full Xcode 26.6 or newer, license accepted |
| macOS SDK | 26.5 or newer; record the actual SDK in every build manifest |
| Metal compiler | Xcode's optional Metal Toolchain component, required by ANGLE's Metal shaders |
| Memory | At least 16 GiB; 32 GiB or more recommended |
| Disk | At least 100 GiB free before fetching; reserve 300 GiB or more for sources, outputs, packages and backups |
| Initial tools | Git, curl, a working Apple developer-tool selection |
| Coordinator | Rust 1.99.1; the entry point downloads a locally installed, SHA-256-pinned toolchain if needed |
| Runtime target | arm64 macOS 13.0+, inherited from the pinned Chromium baseline; this is not yet a tested support matrix |

Chromium uses `xcodebuild` and tools/resources from the full Apple developer distribution.
Command Line Tools are sufficient to compile the Rust coordinator/native updater UI, but are
not the supported full-browser build environment. The Xcode editor, a paid Apple account,
Developer ID signing and notarization are not required.

When Command Line Tools are selected but `/Applications/Xcode.app` exists, the coordinator
automatically uses that app's developer directory for its child processes. An explicit
`DEVELOPER_DIR` or a different full Xcode selection takes precedence. It never changes
the machine-wide selection.

Select a different existing Xcode for this shell:

```sh
export DEVELOPER_DIR="/Applications/Xcode.app/Contents/Developer"
./tools/stainless doctor
```

If Xcode reports that `metal` is missing, restore the official component for that
selected Xcode and rerun diagnostics:

```sh
DEVELOPER_DIR="/Applications/Xcode.app/Contents/Developer" \
  xcodebuild -downloadComponent MetalToolchain
./tools/stainless doctor
```

This compiler is a build-host dependency; installed browsing does not need it.

Install Xcode and accept its license before running bootstrap. The coordinator never installs
Xcode, invokes `sudo`, accepts Apple license terms, or changes your system-wide developer directory.
The generated `.gclient` suppresses Chromium's `mac_toolchain` installer hook; other hooks
still run with the selected existing Xcode. Only the exact previous generated configuration
is upgraded automatically; user-edited checkout configurations are preserved and reported.

## Local commands

```sh
./tools/stainless doctor
./tools/stainless bootstrap --jobs 8
./tools/stainless build --jobs 8
./tools/stainless test
./tools/stainless package
```

`bootstrap` verifies the host, fetches the pinned `depot_tools` and Chromium revisions,
syncs Chromium's DEPS without running hooks, and applies the complete downstream patch stack.
It initializes the pinned tool programs with `ensure_bootstrap` without updating their Git
checkout, and verifies the Python shim needed by GN/Ninja. Credits generation uses that same
pinned interpreter rather than assuming a `depot_tools/python3` executable exists.
`build` runs Chromium's hooks, generates GN files and builds `chrome` and matching `chromedriver`,
then compiles the Rust updater and native Swift menu. Eight jobs are the default to avoid
occupying every core on a shared Mac. `--jobs` accepts 1 through 64.

`test` runs the Rust profile-ownership/migration, protocol, bundled-runtime, archive, installer and recovery
fixtures, compiles the native UI and exercises the offscreen AppKit chooser. The profile fixtures
use owned synthetic browser processes and an isolated HOME, without developer tools on PATH.
The native checks cover metadata/create/launch/recovery transport, bundled-helper selection,
lifecycle, error reporting and keyboard control order without operating the user's desktop.
They also run AppKit recovery controls against the actual production Rust helper in an empty
synthetic HOME without developer tools on PATH: live start acknowledgement, cold admission
after normal fixture-browser exit, verified backup, new-identity restore, original preservation
and corruption failure. Copies/locks and output are real; the browser is an owned synthetic
process, not the rebuilt Chromium UI. Offscreen native renders are under `target/native/`.
These fixtures are synthetic; they do not prove that the
real browser or downloaded package works. The installed browser checks in
[the Phase 1 contract](phase-one.md#release-evidence)
remain mandatory.

`./tools/stainless profiles help` exposes the mode-policy matrix, opt-in metadata registry, foreground per-root
browser ownership and cold backup/copy-only preview import/restore tools. It does not need
Xcode or a Chromium fetch, modify the default preview automatically, or ship the developer CLI inside the app.
See [profile ownership, key/IPC boundaries and migration rules](profile-isolation.md), including
registered-root updater snapshot/recovery and the missing coverage in older installed helpers.
The source adds a native **Stainless Updates > Managed Profiles...** entry point backed by the
bundled Rust helper, not `xtask`. Its recovery window defers this helper's automatic updates
while users close browsers normally, then exposes consented cold backup and clone-only restore.
Packages declare native chooser/recovery capability 1 and browser
managed-modes capability 1; incompatible/missing helpers fail visibly without a developer/default
profile fallback. Existing native/Rust executable hashes, `profile_chooser_version` and
`profile_recovery_version` bind the runtime to the build manifest. This source integration
does not publish a preview or verify installed native chooser/recovery workflows.

Current source also supplies explicit **Open Safe Mode** for a managed Standard identity,
with an explicit **Open Normally** action after safe exit. It preserves saved configuration
and prior session data instead of resetting them; its own tabs are not journaled.
See [startup restrictions and limits](profile-isolation.md#managed-startup-safe-mode).
The seventh patch and bundled helper declare browser `StainlessManagedSafeModeVersion = 1`
and helper `StainlessProfileSafeModeVersion = 1`; both are required before safe launch.
Build/package checks bind `managed_safe_mode_version` and `profile_safe_mode_version` to the
native executable hashes and source manifest. Older pairs are rejected, not silently
launched normally. The new smoke journey requires this new browser build; it does not
revalidate an old candidate or prove installed native operation.

When the exact source and its pinned Node/TypeScript tools are already available, the
settings source fixtures need no additional npm install or browser launch:

```sh
# SOURCE is the exact patched Chromium src directory, outside this repository.
NODE="$SOURCE/third_party/node/mac_arm64/node-darwin-arm64/bin/node"
TYPESCRIPT="$SOURCE/third_party/node/node_modules/typescript/lib/typescript.js"
"$NODE" tests/fixtures/settings_routes.cjs "$TYPESCRIPT" "$SOURCE/chrome/browser/resources/settings"
"$NODE" tests/fixtures/password_import_state.cjs "$TYPESCRIPT" "$SOURCE/chrome/browser/resources/password_manager/passwords_importer.ts"
```

These are synthetic routing/import state-machine checks, not UI or native-import evidence.
An earlier five-patch cache is intentionally preserved rather than silently rewritten:
verify the complete current stack in a separate cache or explicitly review its transition
before a future build. The coordinator must still reject unrecognized source edits.

`./tools/stainless smoke /exact/path/to/Stainless.app` uses the matching locally built ChromeDriver
and disposable profiles to exercise HTTP navigation/find, tabs, downloads, DevTools/PDF,
restart/history and Incognito isolation, and rejection of a synthetic self-signed HTTPS site.
The synthetic browser uses Chromium's mock Keychain and never imports the user's profile.
It never uses `--no-sandbox` or disables TLS validation. It does not automate the user's desktop.
The extended smoke also checks an actual cross-origin out-of-process iframe and native
`sandbox_check` status for its own renderer children. Native UI, bookmarks/session restoration
and clean-host checks remain separate release gates. The synthetic TLS fixture needs the macOS `openssl` command.
Its HTTPS server uses the pinned Python standard library and binds only to loopback,
including on Macs whose bundled LibreSSL cannot bind `s_server` to a specific host.
The smoke also loads IANA's public `https://example.com/` fixture to verify normal trusted
HTTPS after rejecting the self-signed site. This check needs internet access, sends no profile
or repository data, and does not make installed browsing depend on that site.

`package` requires an actual full-browser build and its matching manifest. It copies the complete
non-component app, embeds the updater, emits notices and dependency sources, checks Mach-O
runtime dependencies, preserves required entitlements while applying local ad-hoc signatures
inside-out, runs that local browser smoke against the staged app, and produces:

- `Stainless-VERSION-macos-arm64.dmg`, with an Applications link.
- `Stainless-VERSION-macos-arm64.zip`, the complete app consumed by the updater.
- `stainless-preview.json`, identifying the release, platform, version, build, schema, ZIP name,
  size and SHA-256.
- `SHA256SUMS`, `stainless-build.json`, `Cargo.lock`, Chromium/Rust notices, Rust dependency sources,
  installation instructions, `profile-isolation.md` with native chooser/ownership/recovery limits,
  `degoogling.md` with unavailable features/replacement gaps,
  and explicit unverified release status.

Rust notices/sources cover the updater's resolved arm64 macOS normal/build dependency closure,
not unrelated Android, Windows or test-only crates. The linked Rust standard library's official
copyright notices and MIT/Apache license texts are included separately.
Runtime inspection uses the pinned LLVM `llvm-objdump` load commands, not a dylib's own install
ID or `otool` archive-member parsing of parenthesized Chromium helper names.
Packaging derives the browser and updater's native `.icns` icons from the tracked transparent
[Stainless logo](../chromium/branding/stainless-logo.png) and
[black icon](../chromium/branding/stainless-icon.png), including the 1024px Retina frame.
The original S colors are checked pixel-by-pixel. Both master hashes and the native renderer
hash are recorded/checked before packaging; both PNGs ship as release assets. The build-only
renderer is compiled from `tools/xtask/macos/`, with no new runtime/dependency requirement.
Regenerate the black master after an intentional source-logo change with
`target/native/render-icon chromium/branding/stainless-logo.png chromium/branding/stainless-icon.png 1024`;
then regenerate/review the downstream theme assets rather than leaving stale WebUI logos.

Ad-hoc signatures allow local Apple Silicon execution and detect broken bundle contents. They
are not Developer ID signatures, publisher authentication, or notarization.

Output is under `dist/VERSION/`. Packaging refuses to overwrite a previous candidate. Keep
the prior usable app/package and profile snapshots; do not clean them as ordinary build output.

## Continuous integration

The public repository's [CI workflow](../.github/workflows/ci.yml) runs on pull request creation
and updates, including drafts, pushes to `main`, and manual dispatch. Superseded runs for the
same pull request or branch are canceled. It needs no repository secrets, release credentials,
personal profiles or Google API keys.

The Apple Silicon `macos-26` job checks formatting and Clippy, runs
`./tools/stainless test --jobs 2`, and builds the locked Rust workspace in release mode.
This includes the existing synthetic HTTPS/update, profile ownership/recovery, icon and
offscreen AppKit checks. A separate Ubuntu job audits `Cargo.lock` against current RustSec
advisories with pinned `cargo-audit`. Both use the checked-in Rust toolchain, two Cargo workers
and two Rust test threads. SHA-pinned checkout/cache actions use a read-only token; checkout
does not retain Git credentials. Dependency/build caches are disposable acceleration only.

Hosted CI does **not** fetch or build Chromium, run the source fixtures requiring Chromium's
pinned Node/TypeScript tree, produce a DMG, or publish a release. The opt-in real-payload tests
still need their exact release packages. Full-browser smoke, fresh Stable provenance,
clean-Mac installation and authorized automatic updates remain
[separate release evidence](phase-one.md#release-evidence), not implied by a green CI run.

## Browser command palette

Current source adds **View > Command Palette...**, the browser's three-dot menu entry and
**Command+Shift+P** on macOS. No existing browser/platform binding is replaced; DevTools/app/popup
windows are not palette owners. This is source functionality, not an update to an installed preview.

The search field receives initial focus. Search matches every whitespace-separated term in the
existing localized action label, ignoring case and accents. Empty search lists available commands;
no match shows an explicit empty state. Up/Down enters the native results table, whose arrows,
Home/End and scrolling retain Chromium behavior. Enter or a double-click runs the selected command.
Tab/Shift+Tab moves through search, results and Close; Escape or Close dismisses without dispatch
and returns focus to the owning browser. An already-open palette is reused.

The bounded catalogue contains these existing Chromium actions, **only when supported, visible
and enabled**:

| Group | Actions |
| --- | --- |
| Tabs/windows | New tab, new window, new Incognito window, close tab, duplicate tab, reopen closed tab |
| Page/navigation | Find, back, forward, reload, zoom in/out/reset, focus the address bar |
| Local browser UI | Bookmark this tab, bookmark manager, history, downloads, settings |

Labels, existing shortcut hints and dispatch come from the owning browser's ActionItems and command
controller. The palette is a browser-owned native dialog, not an AppKit sheet or an independent
browser. Disabled/hidden results are removed live; losing a selected result never silently
selects a different command. Activation checks availability again, dismisses the palette window,
then rechecks the **same owner** before normal command dispatch. A stale command fails visibly.
Blocked tabs, other native modals, closing owners and non-normal windows cannot use the palette.
Managed forced Incognito cannot use it to open Standard windows, settings, bookmark management
or restoration; inherited Incognito retains the normal Chromium command semantics, including
the history disclaimer. Safe Mode cannot use it to restore previous sessions, even with stale
controller state. Existing mode, service and backend restrictions remain intact.

Catalogue/search are local and transient. Availability checks read the current tab/window state,
but opening/searching does not enumerate or index browsing records, files, keys or page contents,
persist queries, send telemetry or fetch suggestions. Only explicit activation invokes the
selected existing action. There are no arbitrary URL,
JavaScript, flag, subprocess, filesystem/key, DevTools/CDP, macro or generic renderer-dispatch
commands. Shortcut remapping, macros, Vim navigation and keymap import/export are not implemented.
New Stainless labels follow the existing untranslated resource pattern; action labels reuse
Chromium's translations.

The focused native Chromium fixture target exercises production search/availability, real
tab/find/zoom/library dispatch, stale/hidden commands, owning-browser/modal/lifecycle checks and
keyboard/focus flows, including managed Incognito and Safe Mode. The Mac test launcher uses its
direct support dependencies, not the unrelated WebUI-test resource aggregate. Existing upstream
test sources and the full browser-test target are retained. Build it with eight jobs after
verifying the exact patch stack:

```sh
# SOURCE and DEPOT identify this manifest's isolated patched cache, not another installed browser.
"$DEPOT/autoninja" -C "$SOURCE/out/Stainless" -j 8 stainless_command_palette_tests
# Use an owned, empty disposable HOME. Fixtures are headless and use Chromium's mock Keychain.
CHROME_HEADLESS=1 HOME=/exact/disposable/home "$SOURCE/out/Stainless/stainless_command_palette_tests" \
  --test-launcher-jobs=1 --test-launcher-retry-limit=0
```

These fixtures and the matching full-browser/ChromeDriver smoke do not establish physical
keyboard/screen-reader behavior, installed-native workflows, clean-Mac installation or an
authorized N-to-N+1 update. No installer or browser release is published by this slice.

## Sources, caches and pins

[`chromium/manifest.json`](../chromium/manifest.json) pins Chromium, `depot_tools`, the Rust
bootstrap archive/hash, product version/build and profile schema.
[`chromium/args.gn`](../chromium/args.gn) defines a local, non-component arm64 release build.
The [eight patches](../chromium/patches/) separate identity, browsing defaults/telemetry,
macOS updater integration, translated/native branding, Google-service removal and managed
mode/local-settings integration, temporary managed startup Safe Mode and the native command palette.
The first seven are unchanged by the palette slice. The Safe Mode shared startup selector is
declared in the existing GN constants target; GN arguments, tool pins and profile schema are unchanged.
The [availability guide](degoogling.md) documents the removed protections/features and
replacement issues. PDF-to-Google-Drive is excluded by GN; other service paths are disabled
downstream. Browser sign-in uses the immutable product policy and empty OAuth configuration,
not the non-overrideable derived `enable_dice_support` value. The pinned desktop GN graph
requires supervision code, so Google Family Link activation and remote safe-sites checks are
disabled at runtime rather than setting the unsupported `enable_supervised_users = false`.
Full-browser PDF integration also requires Screen AI code in the build graph; its OCR and
main-content service/model availability gates are disabled downstream.
Speech code remains in the required desktop graph, but recognition/service binding and
caption/translation availability are disabled rather than generating unsupported GN test targets.
Safe Browsing code is also referenced unconditionally by the pinned browser/test graph.
Its standard/enhanced state and request-eligibility gates are disabled downstream, along
with the immutable protection policy, rather than using an unsupported `safe_browsing_mode = 0`.

Chromium history stays outside this repository:

```text
~/Library/Caches/StainlessBuild/
  depot_tools/DEPOT_TOOLS_REVISION/
  chromium/CHROMIUM_REVISION/
    .gclient
    src/
      out/Stainless/
```

Set `STAINLESS_CACHE_DIR` to an absolute, whitespace-free path on an APFS volume to relocate it.
Do not point it inside the Stainless checkout or at another project's Chromium checkout.
The coordinator checks the pinned revision and exact expected tracked changes rather than
resetting a dirty source tree. It never uses `git reset --hard` or deletes unknown source work.
It constructs the expected index by applying each patch in order, so overlapping downstream
hunks are supported without accepting any extra tracked edits.
Cache mutation/builds are locked per Chromium revision so separate worktrees cannot change
the same cached source during compilation. This development cache is separate from the
installed browser's `~/Library/Caches/Stainless` data.

For source-only work before Xcode is installed:

```sh
./tools/stainless fetch --jobs 8
./tools/stainless patch-check
```

Fetching Chromium and its dependencies can take hours and uses substantial bandwidth/disk.
Chromium's pinned DEPS owns its Clang, GN, Ninja/Siso and other engine dependencies. Rust
application dependencies use the committed `Cargo.lock`. No Node, pnpm, WebUI, model or server
dependencies are added because Phase 1 does not use them. `depot_tools` self-updating and its
metrics collection are disabled for coordinated commands.
Google API/OAuth build arguments are explicitly empty, and their standard environment
overrides are removed from coordinated engine commands; no publisher keys are embedded.

Build manifests record the product commit/dirty state, upstream revisions, patch hashes, GN
arguments, Cargo lockfile, resolved Chromium dependencies, actual Xcode/SDK/Rust/Swift versions
and build-host macOS. This is repeatability evidence, not bit-for-bit reproducibility or SLSA.

## Maintaining Chromium

**Source provenance checked 2026-11-13T03:52:19Z**, again after the full build: fresh
[official Mac Stable metadata](https://chromiumdash.appspot.com/fetch_releases?channel=Stable&platform=Mac&num=1)
and the full revision's
[official source VERSION](https://chromium.googlesource.com/chromium/src/+/b8c2be851f62f6a394ec57955d7f31335d7cb6d4/chrome/VERSION?format=TEXT)
agree on **Chromium 158.0.8127.37**.

| Source | Version | Full Chromium revision |
| --- | --- | --- |
| Previous pin / preserved local preview.5 | 158.0.8127.18 | `6449d9f9a6a906edff724b7b220199b3fde4615c` |
| Current pin / official Mac Stable at check time | 158.0.8127.37 | `b8c2be851f62f6a394ec57955d7f31335d7cb6d4` |

The former newer-than-Stable discrepancy is resolved by newer official metadata, not a
downgrade or a gate exception. The
[November 12 early Stable rollout notes](https://chromereleases.googleblog.com/2026/11/early-stable-update-for-desktop_0520415684.html)
announce 158.0.8127.36/.37 and initially say Mac will follow; the later fresh Mac metadata
above identifies its actual Stable revision. Those notes publish no separate security-fix
count. The [November 11 Stable security notes](https://chromereleases.googleblog.com/2026/11/stable-channel-update-for-desktop_01744469280.html)
describe 83 fixes for the preceding 157 release, including critical ServiceWorker, CSS,
Layout, Skia and Accessibility fixes; that is not a claimed count of new fixes in this rebase.
Re-query rather than treating this dated record as current release approval.

The complete six-patch stack and unrelated product, profile-schema, GN, depot_tools and Rust
pins are retained. Patches 0001-0005 are unchanged. Patch 0006 is rebased over upstream's
Gmail-description escaping and settings-view lifetime changes, retaining the same service
removals and local controls rather than restoring the removed Google UI. Changed speech,
Translate, Safe Browsing, model-broker and variations paths were reviewed against the existing
backend denials and immutable policies.

The full `chrome` target, matching ChromeDriver and native helpers built successfully with
eight jobs from clean source commit `e38fc95282ab26b8aaae976fa6e7dbf250bdb42e`.
The build finished at **2026-11-13T03:48:28Z** on Apple Silicon macOS 26.6,
Xcode 27.2 (27B52), SDK 27.2, Metal 32023.922 and Rust 1.99.1.
The exact app is
`~/Library/Caches/StainlessBuild/chromium/b8c2be851f62f6a394ec57955d7f31335d7cb6d4/src/out/Stainless/Stainless.app`;
its adjacent `stainless-build.json` records that commit with `dirty: false`, the full upstream
revision, resolved dependencies and all six patch hashes.

| Built executable | SHA-256 |
| --- | --- |
| Browser launcher | `5b70f251e8084e76a239e1123a5391ab00a4e994e77c5d62e0857176bfda74cd` |
| Active 158.0.8127.37 Stainless Framework | `c65e061f01ab060e34b171278cc6615b0a7afa07ccd902ae135599b6b9f60cf3` |
| Matching ChromeDriver | `cdfc34aad164967309be354d7f22209b804c7d1bb8fda167b51671439172e2c2` |

Actual isolated-browser smoke passes branding and immutable policy checks, removed settings
routes/search and update controls, explicit backend denial, retained local controls, repeatable
import UI with a synthetic service boundary, navigation/find/tabs, download contents, DevTools/PDF,
restart/history/cookies and inherited/managed Incognito isolation. It observes **zero Google-service
URL requests** during the 30-second idle window plus the browsing sequence, verifies a cross-origin
out-of-process iframe and **six owned sandboxed renderers** with `sandbox_check`, rejects the
self-signed HTTPS fixture and loads trusted HTTPS. This remains bounded synthetic-profile evidence,
not all-time network monitoring or a native file import.

A separate disposable managed Standard session reports actual runtime **Chrome/158.0.8127.37**,
keeps DuckDuckGo as the fresh default, excludes Google prepopulation and accepts an explicit
alternative search provider without replacing it with the fallback. The upstream non-official
build default uses dummy `LASTCHANGE`, so CDP/driver revision fields contain zeros. Full-revision
provenance comes from the verified checkout and clean build manifest tied to these actual
runtime/version/binary checks, not from those dummy fields.

The full locked Rust/native checks, formatting, Clippy, release-tool build, source routing/import
fixtures, eight Grit branding tests and `cargo audit --deny warnings` pass. Post-build patch
verification passes against the complete expected tree; no unrecognized-edit or overlap protection
was relaxed. The old preview.5 app and build manifest remain byte-for-byte unchanged.

This source update does not replace published previews or installed browsers, create a new
candidate, or authorize publication. A later candidate needs its own monotonic product
version/build and must preserve the previous `dist/VERSION/` outputs. No package or release was
created. Clean-Mac installation, installed native chooser/recovery, authorized N-to-N+1 updates
and the broader Phase 1 gates remain open.

On November 12, the user explicitly requested a local **preview.5** candidate using the unchanged
Chromium 158 pin. Its [official source VERSION](https://chromium.googlesource.com/chromium/src/+/6449d9f9a6a906edff724b7b220199b3fde4615c/chrome/VERSION?format=TEXT)
was independently verified. The full six-patch browser and matching ChromeDriver build and pass
isolated browser smoke, including settings search, removed deep links, retained local controls
and service-traffic checks. The previous five-patch source/build cache was APFS-cloned before
the reviewed transition, preserving its usable app and incremental build path.
That preserved local experimental candidate remains tied to the old revision; the new source
pin does not revalidate it. It does not change `security-status`, authorize publication,
verify a clean host or prove installed automatic updates.

Coding agents follow the repository's
[Chromium update skill](../.github/skills/chromium-update/SKILL.md) before changing engine
pins, patches or GN arguments and before preparing browser builds, packages or releases.
Ask an agent to "Update Stainless to the latest Mac Stable Chromium" to apply the workflow,
or request a check only to inspect freshness without changing sources.

The skill compares fresh official Mac Stable metadata with both pinned version and revision,
verifies the upstream source version, preserves the downstream stack and requires actual
browser verification. The skill does not add a background watcher or scheduled automation;
source/native CI does not replace daily maintenance and fresh release checks.

Before publishing, and daily while a preview is supported:

```sh
./tools/stainless security-status
cargo audit --deny warnings
```

`security-status` validates fresh official Mac Stable metadata, compares both pinned version
and full revision, and verifies that the official revision's `chrome/VERSION` agrees. It records
the check time and official URLs. Invalid, unavailable or inconsistent provenance blocks the
check rather than reporting the pin current.

An older pin requires a reviewed rebase. A pin newer than the fresh official release is a
provenance discrepancy: by default, preserve it and stop browser build/package/publication work
until resolved, rather than automatically downgrading or reusing an old check. The explicitly
requested local preview.5 experiment above does not waive the publication gate. Version comparison
is numeric, including every component. The 24-hour critical/72-hour other-security-fix response
targets are engineering targets, not an already staffed monitoring service.

To rebase, edit the pin, preserve the old checkout/package, fetch into the new revision's separate
cache directory, resolve patch failures explicitly and run `patch-check`. Never silently discard
local engine changes or apply an unrelated prebuilt browser as a fallback.

### Managed startup Safe Mode source evidence

The full seven-patch `chrome`, matching ChromeDriver and native helpers built from clean
commit `5be4b6fbc221ce02dddcacca53c874828bd525ff`, finishing **2026-11-13T17:21:34Z**.
The browser is in the separately APFS-cloned
`~/Library/Caches/StainlessBuildSafeMode-b4fa9ca0/chromium/b8c2be851f62f6a394ec57955d7f31335d7cb6d4/src/out/Stainless/Stainless.app`.
Its build manifest records `dirty: false`, all seven patch hashes and native/browser Safe Mode
capability **1**. The first six patches, original .37 app/build manifest, older caches and
previous candidates remain unchanged. GN arguments, Chromium/tool/product pins and profile
schema are unchanged.

| Built executable | SHA-256 |
| --- | --- |
| Browser launcher | `5b70f251e8084e76a239e1123a5391ab00a4e994e77c5d62e0857176bfda74cd` |
| Modified Stainless Framework | `d39cf7d813f2a62b29a03298b4b24932fb5e6868f7d3a9d7b47317072b6fe0d8` |
| Matching ChromeDriver | `591c5e0e74479ba0c765b3ba78e878b4f9c771c633168e172e97221b0b1920fa` |

Actual modified-browser smoke passes the Standard -> Safe Mode -> normal journey and an
unsupported mixed-mode startup failure. Safe Mode suppresses an explicit local extension's
content script/background worker, Chromium's restart request and pinned/startup restoration.
It retains the prior session/tab/app file inventory/hashes, saved startup/extension/pinned-tab
configuration, local identity cookies/session storage and prior exit marker. The normal control
and return launches restore the saved tab and run the extension again.
This fixture deliberately replays `--load-extension` and `--restore-last-session`; it is not
proof of native extension installation, saved-extension autoload UI or installed chooser use.
Fixture diagnostics were corrected against Chromium's actual temporary command-line/CDP
extension behavior rather than claiming those loads persist across a restart.

The same smoke retains branding, unavailable Google-service settings/backend checks, local
controls, repeat import, downloads/PDF/history/Incognito, rejected self-signed TLS, trusted
HTTPS, a cross-origin out-of-process iframe and **six owned sandboxed renderers**. It records
zero Google-service URL requests during its bounded 30-second idle plus browsing sequence.
Rust/native checks include the actual AppKit controls and bundled production runtime in an
isolated HOME with no developer tools on PATH; their browser process is synthetic.
Formatting, Clippy, dependency audit, source settings/import fixtures and eight Grit branding
tests pass. No existing tests were removed.

Fresh official Mac Stable/source provenance was rechecked after compilation at
**2026-11-13T17:43:24Z** and still matched **158.0.8127.37** /
`b8c2be851f62f6a394ec57955d7f31335d7cb6d4`. Re-query before any publication.
This is source/browser evidence, not a new preview: no package, release, installation or merge
is authorized or performed here. Physical keyboard/screen-reader, installed native workflows,
clean-Mac, authorized N-to-N+1, licensing/disclosure and broader Phase 1/#4/#6/#7/#8 gates remain open.

## Publication

The repository is now public, but the existing publisher implements the private-preview
contract and deliberately refuses public repositories. Public release licensing/disclosure
decisions in [#27](https://github.com/mbianchidev/stainless/issues/27) and passing fresh Stable
provenance remain required. Source/native CI adds no automatic publication path and does not
waive either gate. The command below documents the existing private-preview path, not a
working public-release command.

Commit/push the exact source first, then build from that clean revision. A dirty-source package
cannot be published. A recent `cargo-audit` command and authenticated `gh` with release-write
access are required on the publisher machine only:

```sh
./tools/stainless publish dist/0.1.0-preview.4
```

Publication verifies final hashes, source identity, current Chromium stable and dependency
advisories; checks that the repository is still private; uploads all assets to a draft release;
then makes it a private prerelease candidate. Failures leave a draft rather than replacing an
existing release. Publisher credentials are never bundled or copied into the updater.

Candidate publication is not a Phase 1 completion claim. Record actual download/install,
N-to-N+1 and failure-recovery evidence in the epic before declaring the phase shipped.
Source/native CI does not satisfy those installed-release checks.

The updater's opt-in artifact tests exercise real release ZIPs, native app exchange and
profile backup/recovery using only synthetic data:

```sh
STAINLESS_TEST_PREVIOUS_PACKAGE_DIR="$PWD/dist/0.1.0-preview.1" \
STAINLESS_TEST_PACKAGE_DIR="$PWD/dist/0.1.0-preview.2" \
  cargo test --locked -p stainless-updater --test release_payload -- \
    --ignored --test-threads=1
```

These tests inject startup success/failure signals explicitly. They do not claim a real
network download, native browser restart or user-configured private authorization.

The ordinary Rust suite also exercises the production HTTPS discovery/download client through
a loopback-only synthetic GitHub proxy. It generates an ephemeral CA/certificate using the
existing macOS OpenSSL and system Python, trusts that CA only in the individual test client,
and never reads Keychain or real credentials:

```sh
cargo test --locked -p stainless-updater network::tests
```

Coverage includes scoped synthetic authorization, cross-origin credential removal, TLS and
redirect rejection, offline/denied access, metadata size boundaries, malformed/incompatible
feeds, interrupted/oversized downloads and hash rejection without app/profile changes.
The production updater gains no alternate feed, certificate override or test mode.
These fixtures are not real private-release or clean-host evidence. Follow the
[installed-update verification procedure](update-verification.md) for those gates.
