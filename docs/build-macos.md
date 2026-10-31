# Building on Apple Silicon macOS

The local coordinator builds Chromium's full `chrome` target. It does not download and
rebrand an existing browser, use Electron, or substitute a system WebView.

**Status:** the complete Chromium source build and isolated browser smoke checks pass on
Apple Silicon macOS 26.6 with Xcode/SDK 27.2 and the Metal Toolchain 27B47.
Downloadable packaging, clean-user installation and installed updates remain release gates.
Full Xcode is a build prerequisite, not an end-user dependency.

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

`test` runs the Rust protocol, archive, installer and recovery fixtures and compiles the native
UI. These fixtures are synthetic; they do not prove that the real browser or downloaded package
works. The installed browser checks in [the Phase 1 contract](phase-one.md#release-evidence)
remain mandatory.

`./tools/stainless smoke /exact/path/to/Stainless.app` uses the matching locally built ChromeDriver
and disposable profiles to exercise HTTP navigation/find, tabs, downloads, DevTools/PDF,
restart/history and Incognito isolation, and rejection of a synthetic self-signed HTTPS site.
The synthetic browser uses Chromium's mock Keychain and never imports the user's profile.
It never uses `--no-sandbox` or disables TLS validation. It does not automate the user's desktop.
Native UI, bookmarks/session restoration, actual sandbox inspection and clean-host checks remain
separate release gates. The synthetic TLS fixture needs the macOS `openssl` command.
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
  installation instructions and explicit unverified release status.

Rust notices/sources cover the updater's resolved arm64 macOS normal/build dependency closure,
not unrelated Android, Windows or test-only crates. The linked Rust standard library's official
copyright notices and MIT/Apache license texts are included separately.
Runtime inspection uses the pinned LLVM `llvm-objdump` load commands, not a dylib's own install
ID or `otool` archive-member parsing of parenthesized Chromium helper names.

Ad-hoc signatures allow local Apple Silicon execution and detect broken bundle contents. They
are not Developer ID signatures, publisher authentication, or notarization.

Output is under `dist/VERSION/`. Packaging refuses to overwrite a previous candidate. Keep
the prior usable app/package and profile snapshots; do not clean them as ordinary build output.

## Sources, caches and pins

[`chromium/manifest.json`](../chromium/manifest.json) pins Chromium, `depot_tools`, the Rust
bootstrap archive/hash, product version/build and profile schema.
[`chromium/args.gn`](../chromium/args.gn) defines a local, non-component arm64 release build.
The [three patches](../chromium/patches/) separate identity, browsing defaults/telemetry and
the macOS updater integration.

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

Before publishing, and daily while a preview is supported:

```sh
./tools/stainless security-status
cargo audit --deny warnings
```

`security-status` fails when the Chromium pin is superseded by Mac stable. Review upstream
security release notes, update the exact version/revision and minimal patches, then build/test
again. The 24-hour critical/72-hour other-security-fix response targets are engineering targets,
not an already staffed monitoring service.

To rebase, edit the pin, preserve the old checkout/package, fetch into the new revision's separate
cache directory, resolve patch failures explicitly and run `patch-check`. Never silently discard
local engine changes or apply an unrelated prebuilt browser as a fallback.

## Private publication

Commit/push the exact source first, then build from that clean revision. A dirty-source package
cannot be published. A recent `cargo-audit` command and authenticated `gh` with release-write
access are required on the publisher machine only:

```sh
./tools/stainless publish dist/0.1.0-preview.1
```

Publication verifies final hashes, source identity, current Chromium stable and dependency
advisories; checks that the repository is still private; uploads all assets to a draft release;
then makes it a private prerelease candidate. Failures leave a draft rather than replacing an
existing release. Publisher credentials are never bundled or copied into the updater.

Candidate publication is not a Phase 1 completion claim. Record actual download/install,
N-to-N+1 and failure-recovery evidence in the epic before declaring the phase shipped.
No live or dormant CI workflow is required or included.
