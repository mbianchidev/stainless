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

Modified-browser verification is pending. No bounded test proves that no future
code, arbitrary extension, compromised OS or user-directed transfer can ever
disclose a visited URL. Physical/installed workflows and the outstanding
[release gates](phase-one.md#release-evidence) remain open.
