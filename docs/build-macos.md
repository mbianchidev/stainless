# Building on Apple Silicon macOS

The local coordinator builds Chromium's full `chrome` target. It does not download and
rebrand an existing browser, use Electron, or substitute a system WebView.

**Status:** the published preview baseline built and passed isolated browser smoke checks on
Apple Silicon macOS 26.6 with Xcode/SDK 27.2 and the Metal Toolchain 27B47.
The unsigned DMG/ZIP/feed are published as private preview candidates. DMG install/eject
and packaged browsing pass locally; clean-Mac and automatic-update gates remain open.
Full Xcode is a build prerequisite, not an end-user dependency.
The current sixth-patch mode/settings changes and native managed-profile chooser/recovery are source-only
and have not passed a new browser build/UI smoke. The provenance discrepancy below blocks that
work, not the independent Rust/native builds and synthetic checks.

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
does not publish or verify a new browser preview.

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

## Sources, caches and pins

[`chromium/manifest.json`](../chromium/manifest.json) pins Chromium, `depot_tools`, the Rust
bootstrap archive/hash, product version/build and profile schema.
[`chromium/args.gn`](../chromium/args.gn) defines a local, non-component arm64 release build.
The [six patches](../chromium/patches/) separate identity, browsing defaults/telemetry,
macOS updater integration, translated/native branding, Google-service removal and managed
mode/local-settings integration. The first five remain unchanged by the settings slice.
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

The working source pin is **Chromium 158.0.8127.18**, verified against official Mac Stable
metadata and source `chrome/VERSION` on November 7, 2026. See the
[early Stable rollout notes](https://chromereleases.googleblog.com/2026/11/early-stable-update-for-desktop.html)
and [preceding Stable security fixes](https://chromereleases.googleblog.com/2026/11/stable-channel-update-for-desktop.html).
This source upgrade does not replace published previews or installed browsers; their engine
versions remain recorded in their own build manifests.

**Current blocker, checked 2026-11-12T00:43:31Z:** fresh
[official Mac Stable metadata](https://chromiumdash.appspot.com/fetch_releases?channel=Stable&platform=Mac&num=1)
reports **157.0.8103.79 / `6dc9bc3bdaea5c35ed6d41fceab2f7a3a2227c4f`**.
Its official source `chrome/VERSION` was verified. The unchanged pin is newer:
**158.0.8127.18 / `6449d9f9a6a906edff724b7b220199b3fde4615c`**.
Browser build/package/UI verification stays blocked until that discrepancy is resolved,
without downgrading or substituting an installed browser. Re-query rather than treating this
dated record as current release evidence.

Coding agents follow the repository's
[Chromium update skill](../.github/skills/chromium-update/SKILL.md) before changing engine
pins, patches or GN arguments and before preparing browser builds, packages or releases.
Ask an agent to "Update Stainless to the latest Mac Stable Chromium" to apply the workflow,
or request a check only to inspect freshness without changing sources.

The skill compares fresh official Mac Stable metadata with both pinned version and revision,
verifies the upstream source version, preserves the downstream stack and requires actual
browser verification. It does not add a background watcher, scheduled automation or CI;
daily maintenance and release checks still need to be run.

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
provenance discrepancy: preserve it and stop browser build/package/publication work until
resolved, rather than automatically downgrading or reusing an old check. Version comparison
is numeric, including every component. The 24-hour critical/72-hour other-security-fix response
targets are engineering targets, not an already staffed monitoring service.

To rebase, edit the pin, preserve the old checkout/package, fetch into the new revision's separate
cache directory, resolve patch failures explicitly and run `patch-check`. Never silently discard
local engine changes or apply an unrelated prebuilt browser as a fallback.

## Private publication

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
No live or dormant CI workflow is required or included.

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
