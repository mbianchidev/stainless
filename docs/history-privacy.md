# Saved browsing history stays local

The current source keeps saved browsing history out of browser-controlled
telemetry, Google sync/cloud history, history-capable extensions and diagnostic
uploads. This is a fixed product restriction, not a consent toggle, a telemetry
sampling choice or a setting that old preferences can turn back on.

**This is source functionality, not a change to an installed preview.** No new
browser or installer is published by this change.

## Enforced paths

| Path | Restriction |
| --- | --- |
| Metrics and URL-keyed telemetry | Existing immutable telemetry/URL-collection policies and consent denial remain. Chromium's `force-enable-metrics-reporting` override is removed at the earliest browser startup callback, before metrics services are initialized. |
| Google sync and cloud history | Browser sign-in/sync remain unavailable. The separate cloud-history service has no profile instance, including normal and private contexts. Ordinary local History remains available. |
| Saved-page icons | The browser's large-icon provider has no remote fetcher and refuses remote fallback for saved-page URLs. Existing local favicons and local fallback styles remain available; ordinary page-requested icons are separate. |
| Extension history access | `history`, `topSites`, `sessions`, `downloads`/`downloadsInternal` and extension `debugger` APIs are unavailable in browser and renderer availability checks, including old grants and experimental-extension flags. Required permissions block extension admission; optional requests are denied. Per-extension/update-source settings cannot relax this restriction. |
| Crash reports | No browser crash-upload URL is supplied. On macOS, upload denial is enforced even over OS-managed metrics consent and an explicit upload-consent request. Local crash diagnosis is not disabled. |
| Feedback and queued diagnostics | The Chrome feedback uploader is not created, so old queued reports are not loaded or sent. Submission fails explicitly before collecting diagnostic attachments. Actor-journal feedback also refuses a missing uploader. |
| Performance traces | Remote background-trace recording and explicit native trace-upload requests are denied. Local traces and local DevTools remain separate. |

Extension compatibility changes are intentional: extensions that require those
permissions cannot run, and previously granted optional capabilities no longer
expose their APIs. This includes extension download managers and extension
debuggers. Chromium's own DevTools, ordinary website navigation, native
downloads, local history/search/delete, local profile recovery and explicit
supported bookmark/password/keymap transfers are not removed.

The [resource overview](resource-overview.md) continues to use only transient
local task/provider state. It has no history query, upload, export, telemetry or
external helper interface. Its test logs contain fictional fixture titles only,
not real browsing data.

## What this does not mean

Sites necessarily receive the current URLs you open, requests their pages make
and data you deliberately submit. Normal HTTP referrers retain Chromium's
existing policy, as explicitly requested. Browser updates and build-time
Chromium freshness checks also use the network without sending history.

An extension separately allowed to inspect live pages can observe those pages
and make its own records. Denying saved-history APIs is not a promise that
arbitrary extension code cannot observe future browsing. Likewise, a website
can record your visits to it. Review extension/site permissions.

This is not a network air gap, encrypted history, secure deletion, anonymity or
same-user OS isolation. It cannot stop a user from copying/uploading a file or
screenshot, a trusted native host from reading plaintext profile files, another
same-user process or modified browser from reading them, or OS-level diagnostics
outside Stainless's control. Local backups and diagnostics can still contain
sensitive data and must be handled accordingly.

## Verification

Source-informed native Chromium journeys exercise fictional local history,
restart persistence, normal referrers, blocked extension installation and
optional requests, simulated old grants, real renderer API availability, forced
metrics permission updates, unavailable cloud-history/feedback services and
native crash/trace policy. Synthetic telemetry endpoints bind only to loopback;
tests do not use installed profiles, real browsing data, Keychain or desktop
input.

These journeys run in the existing `stainless_command_palette_tests` target;
use `--gtest_filter='StainlessHistoryPrivacy*'` for this change. The full retained
native suite and exact modified-browser smoke remain separate from hosted
Rust/native CI and installed-release evidence.

**Verified source, 2026-11-15:** the full twelve-patch browser, matching
ChromeDriver and native fixture built from clean source
`6484270ab4b1c7bd6e57b0c5664d6d814831b0fa` on Apple Silicon macOS 26.6,
Xcode/SDK 27.2, with eight build jobs. That verified source/app is preserved in
`~/Library/Caches/StainlessBuildHistoryVerified-b98f3e6f/`; its
`chromium/REVISION/src/out/Stainless/stainless-build.json` records that commit
with `dirty: false` and all twelve matching patch hashes. The first eleven
patch files, upstream/tool/product/profile-schema pins and GN arguments are
unchanged. The prior eleven-patch source/app/manifest are preserved in
`~/Library/Caches/StainlessBuildResourceOverviewBeforeHistory-b98f3e6f/`;
the earlier keymap cache is also untouched.

The complete native suite passes **42 final tests plus five automatic restart
PRE stages**, including the retained resource/palette/shortcut/keymap journeys.
A fresh privacy-only repetition passes **seven final tests plus one PRE stage**.
There are no launcher retries. Required installation is rejected separately for
all five blocked permissions; valid optional requests are denied, and all five
simulated prior active grants remain unavailable in real renderer/native
availability checks. Unrelated extension-local storage still works. The
crash/trace checks exercise native product policy directly, not a forced crash
or a real diagnostic upload.

The exact app/driver smoke also passes browsing, native downloads, DevTools/PDF,
settings and repeat import, local-history restart and private isolation, managed
Safe Mode and normal return, rejected self-signed TLS, trusted HTTPS, an
out-of-process iframe and **six owned sandboxed renderers**. It records
**zero Google-service URL requests** over a 30-second idle plus browsing, without
ChromeDriver's background-network suppression masking the result. The retained
97 Rust tests and offscreen native chooser/recovery, formatting, strict Clippy,
fresh RustSec audit and post-build twelve-patch verification pass. Five opt-in
real-payload Rust tests remain ignored; final native GN header dependencies pass.

Fresh official [Mac Stable metadata](https://chromiumdash.appspot.com/fetch_releases?channel=Stable&platform=Mac&num=1)
and [source VERSION](https://chromium.googlesource.com/chromium/src/+/b8c2be851f62f6a394ec57955d7f31335d7cb6d4/chrome/VERSION?format=TEXT)
still agree on **158.0.8127.37 /
`b8c2be851f62f6a394ec57955d7f31335d7cb6d4`** at
**2026-11-15T11:47:45Z**, after the build.

| Twelve-patch binary | SHA-256 |
| --- | --- |
| Browser launcher | `5b70f251e8084e76a239e1123a5391ab00a4e994e77c5d62e0857176bfda74cd` |
| Stainless Framework | `9f107d63b7f9c02b1c89c068b8bba4505b902d097709fca06f3a2022dba00940` |
| Matching ChromeDriver | `591c5e0e74479ba0c765b3ba78e878b4f9c771c633168e172e97221b0b1920fa` |
| Native fixture | `c47d0f7a4d2f9f928f6179a1036d8325420cebc7cd0cd1fa699aa038bf42297f` |

Headless logs retain virtual-display fallback, ignored cloud-policy, existing
actor-metadata `NOTIMPLEMENTED` and occasional child-reaping diagnostics, plus
expected forced-telemetry and denied-extension warnings. The pre-existing
[upstream unit-suite gap](resource-overview.md#verification) is unchanged.
Owned fixture homes, browser processes and loopback/TLS fixtures are cleaned up;
usable caches and verification logs are retained.

This is bounded source/browser evidence, not a proof that future code,
arbitrary extensions, a compromised OS or user-directed transfers can never
disclose a visited URL. Physical/installed workflows, public licensing/disclosure,
authorized updates and the outstanding
[release gates](phase-one.md#release-evidence) remain open. That source-only run
produced no installer or installed-preview update. The separately versioned
[local development candidate](install-macos.md#local-development-candidate)
does not publish a release or update an installed browser automatically.
