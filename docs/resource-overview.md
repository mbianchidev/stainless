# Local resource overview

Current source adds **View > Resource Overview...** and the same entry in the
browser's three-dot menu on macOS. It is a browser-owned native, read-only view,
not a replacement shell or the complete
[#29 resource manager](https://github.com/mbianchidev/stainless/issues/29).
This source slice does not update the installed preview or publish a browser.

## Metrics and attribution

The view reuses Chromium's task-manager providers, process groups and background
CPU/memory sampling. It does not scrape the OS process list or inspect other apps.
There is one row per represented process, even when multiple visible tabs share it.

| Column | Meaning |
| --- | --- |
| Process / this window's tabs | Generic Browser, GPU or Utility type, or the titles of this window's tabs associated with a renderer process. Untitled tabs do not fall back to URLs. |
| Process ID | Chromium's actual browser-owned process identifier. It is not an editable input or process-control interface. |
| Attribution | Browser/GPU/utility totals are shared across this browser instance. Renderer totals are shared process measurements with approximate tab attribution, never evenly divided among tabs. |
| CPU (%) | Chromium's last completed background sample for the whole process. 100% represents one CPU core; multithreaded processes can exceed 100%. |
| Memory footprint (MiB) | Chromium's process footprint in mebibytes, rounded to one decimal place. 1 MiB is 1,048,576 bytes. It is not an exact tab allocation, JS heap, or total physical RAM impact. |

Renderer rows can include out-of-process frames and cached renderers associated
with an owned tab. Their process totals can also include other tabs, workers and
engine overhead. Browser, GPU and utility processes cannot be allocated reliably
to individual windows or profiles. No complete profile/browser total is claimed;
unrepresented background/extension/worker processes are not silently counted as
zero. Dedicated extension, workspace, agent and model attribution remains later work.

**Collecting...** means no completed Chromium sample has reached the view yet.
**Unavailable** means there is no valid measurement, not zero. A tab without a
current renderer remains visible with unavailable CPU/memory and an explanation;
opening the overview does not load or resume it. Use that tab's ordinary loading
or reload controls, or keep the overview open for a sample.
Persistent unavailable readings can be retried by closing and reopening the view.
CPU availability is carried through the existing sampler rather than its legacy
zero-on-error getter, which remains compatible for prior consumers. A missing
process in a memory-dump response clears its prior footprint instead of displaying
that stale value as a fresh reading.

## Privacy, ownership and lifetime

Only tabs in the exact owning window and its actual Chromium profile context
contribute titles. Normal views do not display other windows' or Incognito titles,
URLs or contents. Private views do not display normal or other private windows'
tab details. Profile association uses Chromium's tab IDs and live renderer browser
contexts, not profile-name matching or an external metadata registry.

Shared Browser/GPU/Utility **numbers** can include other normal or private profiles
in the same browser instance, but their task titles and URLs are never shown.
Other managed roots' separate browser instances are outside this task-manager
instance and are not inspected. This is not encryption or an OS-enforced
same-user isolation claim.

Everything stays in the browser process and transient native view state. There
is no history persistence, export, telemetry, account, cloud endpoint, helper
protocol, root privilege or new runtime service. The view requests only CPU and
memory refreshes at Chromium's one-second observer cadence. Closing it or its
owner removes its tab/provider observers and clears its rows. If another resource
view or Chromium Task Manager is open, Chromium retains only the refresh work
required by those remaining observers.
The browser's separate [saved-history privacy restrictions](history-privacy.md)
also apply; the overview is not a history collector.

Tab insertion, title changes, movement, renderer task replacement and removal
update the native rows. Selection follows the same task identity; removing a
selected task does not select an unrelated process. Closing an owner closes its
view. Opening, reopening and menu dispatch retain normal-window, profile,
blocked-tab, modal and closing-owner restrictions. Incognito and managed startup
Safe Mode can inspect their own resources without changing their mode or state.
The fixed keyboard/keymap action schema is unchanged.

## Navigation and limits

The table receives initial focus. Arrow keys, Home/End and scrolling use
Chromium's native table behavior. Tab/Shift+Tab move between the table and Close;
Escape or Close dismisses and requests focus restoration to the live owner.
Headers, the table name and unavailable status use native accessibility and
localized resources. New Stainless labels follow the existing untranslated
resource convention; physical keyboard/VoiceOver evidence remains separate.

There is no tab activation, termination, force-close, suspension, hibernation,
automatic intervention or hard limit in this view. Network, storage, battery,
extension, workspace, agent and model metrics and dashboard adapters are not
implemented here. No numerical performance or reliability gate is introduced.
Ordinary browsing is unchanged. Chromium's existing Task Manager keeps its
entry points and controls.
**The upstream Task Manager is a separate browser-instance-wide surface; it does
not inherit this overview's window-scoped detail filtering.**

## Verification

The focused native resource journeys are part of the existing
`stainless_command_palette_tests` target. They use fictional loopback pages,
owned headless Chromium profiles and the real task-manager providers, not a
mock table, installed Chrome, desktop input or the real Keychain.
See the [native build/run commands](build-macos.md#browser-command-palette).
Use `--gtest_filter='StainlessResourceOverview*'` for just this slice.

**Verified eleven-patch source, 2026-11-15:** the complete browser, matching
ChromeDriver and native fixture built from clean source
`9b64a538cc330787f84de9197c533af2e2b77840` on Apple Silicon macOS 26.6,
Xcode/SDK 27.2, with eight build jobs. That source/app and its
`stainless-build.json` are now preserved in the APFS-cloned
`~/Library/Caches/StainlessBuildResourceOverviewBeforeHistory-b98f3e6f/`,
recording that commit with `dirty: false` and all eleven patch hashes.
The separately verified [twelve-patch history follow-up](history-privacy.md#verification)
also reran all seven resource journeys and the full retained native suite, and is
preserved in `StainlessBuildHistoryVerified-b98f3e6f/`. The original
`StainlessBuildResourceOverview-b98f3e6f/` cache is reused for versioned local candidates.
All ten prior patch files and the earlier keymap cache/framework remain unchanged.
No upstream, toolchain, GN argument, product or profile-schema pin changed.

Fresh official [Mac Stable metadata](https://chromiumdash.appspot.com/fetch_releases?channel=Stable&platform=Mac&num=1)
and [full source VERSION](https://chromium.googlesource.com/chromium/src/+/b8c2be851f62f6a394ec57955d7f31335d7cb6d4/chrome/VERSION?format=TEXT)
still agree on **158.0.8127.37 /
`b8c2be851f62f6a394ec57955d7f31335d7cb6d4`** at
**2026-11-15T02:42:46Z**, after the browser build.

All **seven resource journeys pass four runs each** without launcher retries,
including their run inside the complete native suite: **28 resource passes**.
The full suite passes all **35 final tests plus four automatic restart PRE
stages**, retaining palette, shortcut and keymap behavior. Real loopback
workloads allocate fictional buffers in two tabs sharing one actual renderer,
then add an out-of-process iframe. The displayed process IDs, CPU readings and
MiB conversion match Chromium's live providers exactly; the shared renderer
appears once with only owned titles. Other windows/profiles/private titles and
URLs stay hidden. Live title/move/removal, no-renderer/failed-sample states,
native keyboard/focus, owner close, Safe Mode and forced private observation pass.
These are measurements and behavior checks, not machine-wide CPU thresholds.

The exact app/driver smoke also passes ordinary browsing, settings and repeat
import, removed-service controls/backend denial, persistence and private
isolation, managed Safe Mode and normal return, rejected self-signed TLS,
trusted HTTPS, an out-of-process iframe and **six owned sandboxed renderers**.
It records **zero Google-service URL requests** during 30 seconds of idle plus
browsing. The 97 locked Rust tests, existing offscreen native chooser/recovery,
formatting, Clippy, release-tool build, source settings/import fixtures, four
targeted Grit tests, RustSec audit and post-build strict patch check pass.

| Retained eleven-patch binary | SHA-256 |
| --- | --- |
| Browser launcher | `5b70f251e8084e76a239e1123a5391ab00a4e994e77c5d62e0857176bfda74cd` |
| Stainless Framework | `115fad2f68e5a9a41c8e841ddfad5ed90c1840ae34750cbef42619b4372cedd2` |
| Matching ChromeDriver | `591c5e0e74479ba0c765b3ba78e878b4f9c771c633168e172e97221b0b1920fa` |
| Native fixture | `596a5aea310c47b254fffb25ad8073c63f9ed0a42a4bc0c882ba3830076842d3` |

Headless logs retain virtual-display/CVDisplayLink fallback, ignored cloud-policy,
existing actor-metadata `NOTIMPLEMENTED`, occasional child-reaping diagnostics
and expected unavailable-command/sampler warnings. A separate probe of the
upstream `unit_tests` object aggregate pulls password-manager WebUI tests that
still expect Google-service controls removed by unchanged patch 0006; those
tests fail 107 TypeScript checks. Their sources/targets remain intact, and this
is not a claim that the full upstream unit suite passes.
Five opt-in real-payload Rust tests still require release packages and remain
ignored. Owned fixture homes, browsers and loopback servers are cleaned up;
the build and verification logs are retained.

Source/browser checks do not satisfy installed native workflows, physical
keyboard/VoiceOver accessibility, clean-Mac installation, authorized updates,
publication/licensing or the full Phase 1/#4/#6/#7/#8/#29 gates. That eleven-patch
source run produced no installer or installed-preview update; see the separately
versioned [local development candidate](install-macos.md#local-development-candidate).
