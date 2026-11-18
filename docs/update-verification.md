# Verifying automatic updates on macOS

This is the installed-browser acceptance procedure for [#6](https://github.com/mbianchidev/stainless/issues/6)
and [#37](https://github.com/mbianchidev/stainless/issues/37), not a claim that those gates pass.
Record results in [epic #1](https://github.com/mbianchidev/stainless/issues/1) and track the
run in [#46](https://github.com/mbianchidev/stainless/issues/46). Keep every unexecuted,
failed or unsupported gate open.

## Current eligible pair

Both releases are public, so downloading them needs no GitHub sign-in:

| Role | Release | Chromium | Download |
| --- | --- | --- | --- |
| N | [0.1.0-preview.4](https://github.com/mbianchidev/stainless/releases/tag/stainless-preview-v0.1.0-preview.4), build 4 | 156.0.8078.80 | `Stainless-0.1.0-preview.4-macos-arm64.dmg` |
| N+1 | [0.1.0-preview.8](https://github.com/mbianchidev/stainless/releases/tag/stainless-preview-v0.1.0-preview.8), build 8 | 158.0.8127.37 | Updater ZIP, discovered automatically |

Preview.4's updater lists published prereleases newest first and takes the first
`stainless-preview-v*` release newer than itself, then validates its
`stainless-preview.json` feed. Preview.8's feed matches that contract: schema 1, preview
channel, profile schema 1, macOS arm64, minimum macOS 13.0 and the ZIP's size and SHA-256.
No new publication is needed for the first installed run. Preview.4 lacks later Chromium
security fixes: keep its browsing to synthetic/local content.

This first run cannot show startup discovery: saving the token immediately checks, and
preview.8 is already published (step 4 below). Preview.4's helper also predates
journal-format-2 managed snapshots. Once the test account runs an authorized preview.8,
publish a later N+1 and repeat the procedure with preview.8 as N to cover startup
discovery and managed roots.

## Provision the two environments

| Environment | Required boundary |
| --- | --- |
| Publisher | Only needed for a new N+1: the existing build host, exact committed source, release-write access through `gh`, current official Mac Stable evidence and dependency audit |
| Installed client | Separate supported Apple Silicon Mac or a genuinely separate logged-in macOS user, its own home/profile/Keychain, no checkout or developer dependencies, and a user-writable `~/Applications` installation |

A temporary directory, sanitized `PATH`, changed `HOME`, headless browser, Linux VM or another
profile belonging to the build user is not a separate clean macOS environment. Do not create
accounts, switch the user's desktop, alter their installed browser/profile or reuse unrelated
VMs without authorization. If no suitable environment is provisioned, finish source work and
record the installed-client gates as blocked.

The updater still requires a read-only GitHub token, even though the repository is public.
The client operator must provision their own expiring fine-grained token with
**Contents: Read-only**, limited to **Only select repositories > stainless** (or, when the
token's owner is not the repository owner, **Public repositories**), using the
[native authorization flow](install-macos.md#automatic-private-updates). Enter it directly
in **Stainless Updates > Authorize Private Updates > Save in Keychain** on the test account.
Do not pass it through chat, environment variables, process arguments, files or evidence.
Publisher `gh` access is not client authorization. A successful authenticated request alone
does not prove that the token has no write permissions; review its configured permissions.

## Prepare a newer version pair

Only needed when the current pair is no longer eligible or a later N+1 is required.
Releases go through [`scripts/release.sh`](releasing.md) on the build host, which runs the
[Chromium maintenance](build-macos.md#maintaining-chromium) gate, bumps a fresh monotonically
increasing version/build, keeps `profile_schema: 1` and publishes through release CI.
`security-status` must verify both the version/revision and the official source `chrome/VERSION`.
Unavailable or inconsistent upstream evidence blocks a new candidate. If the pin is newer
than fresh official Mac Stable metadata, stop and resolve it; never downgrade or reuse an
older successful check. Never overwrite N or republish different bytes under its version.

N and N+1 must both be actual published releases with matching DMG, ZIP, feed, checksums,
source revision and build manifest. Install N before making N+1 available if possible.
An older N is a short-lived synthetic update fixture, not a recommendation to browse with a
superseded engine. Keep its browsing limited to synthetic/local content.

## Exercise the installed updater

1. In the clean test account, download N's actual DMG and `SHA256SUMS` from its
   release page. Verify the DMG with `shasum -a 256`, mount it, copy `Stainless.app` into
   `~/Applications`, and eject it. Follow the [unsigned-app guidance](install-macos.md)
   without disabling Gatekeeper globally.
2. Launch that installed app normally, without `--headless`, `--user-data-dir` or developer
   flags. Those modes intentionally do not start the production updater or emit its
   startup-health signal. Confirm the installed version/build and default Stainless profile.
3. Use only synthetic state: create a bookmark named `Stainless update fixture` targeting
   `about:blank#stainless-bookmark`, open two distinct synthetic tabs, and select
   **Settings > On startup > Continue where you left off**. Quit/relaunch N once and
   confirm this state before testing the update.
4. Configure the test user's read-only authorization in the native menu. Saving it starts
   an immediate automatic check. When N+1 is already published, as with the
   [current pair](#current-eligible-pair), that check discovers, downloads and stages it:
   record this as authorization-triggered discovery, not startup discovery. To evidence
   startup discovery, authorize N, let its check finish with nothing newer, quit N and its
   updater, publish N+1, then relaunch N normally. Never press **Check for Updates** for
   either case; record any manual check separately. Observe a four-hour periodic check
   separately if claiming the periodic gate.
5. Record the staged target and visible status. Do not replace the app manually or point
   the client at a local payload directory. The production client must obtain the actual
   published feed and ZIP over validated HTTPS and verify the complete payload.
6. Choose **Restart to Update**. Resolve any browser quit prompt normally; never force-close
   user work. Confirm N+1 launches from the installed path, retains the exact bookmark and
   both tabs, and can navigate normally. Allow its real 30-second startup-health signal;
   never write a marker or inject `--observed-start` to manufacture this evidence.
7. Confirm the updater reports a healthy transaction, the installed bundle passes
   `codesign --verify --deep --strict`, and the previous app/profile snapshot is retained.
   Ad-hoc integrity verification is not publisher signing or proof of native sandboxing.
   Record actual installed sandbox/site-isolation evidence separately or keep that gate open.

Record the actual actions and observations, not just file existence or an updater exit code.
A healthy startup marker establishes responsive startup, not bookmark/session correctness
or the complete native browsing matrix. Screenshots, when needed, must show only the
synthetic test environment.

Managed-root coverage is an additional installed-client gate. Use only an authorized setup
with already explicitly registered synthetic roots; record its provisioning separately without
silently importing a real profile or adding developer dependencies to the clean client.
Confirm that the helper preparing the update uses journal format 2, then record each root's
synthetic bookmarks/session, registry compatibility floor, snapshot and failed-state retention.
Exercise interruption/retry across root, registry and app restoration. Older helpers provide
no historical managed snapshots; a new payload alone cannot satisfy this gate. If that
fixture/setup or the actual version pair is unavailable, leave multi-root installed evidence open.

## Failure and recovery evidence

| Risk | Existing isolated coverage | Additional installed-client evidence |
| --- | --- | --- |
| Offline, HTTP 401/403 | Production HTTPS client through loopback fixtures; failures are errors, not "current" | Browser remains usable without authorization/access; correct visible error and retry after restoring access |
| TLS failure, unsafe/excessive redirects, credential forwarding | Synthetic private CA; untrusted certificates rejected; cross-origin CDN requests must contain no authorization | Actual GitHub release download without certificate overrides or publisher-token reuse |
| Malformed/oversized metadata, stale build, wrong target/schema | Production discovery and shared validation fixtures | Any operator-run negative case must leave the installed app and synthetic state unchanged |
| Truncated/oversized download, wrong hash | Production streamed download and hash/staging rejection; synthetic app/profile sentinels preserved | Interrupted real download/retry without a partial installation |
| Live browser lock, denied app-folder permission, interrupted exchange | Native filesystem transaction fixtures | Visible installation failure/retry and usable retained installation in the clean account |
| Failed launch/profile recovery | Native app/profile exchange and real release-payload tests, with injected startup signals | Actual failed-launch recovery with matching prior app/profile and preserved synthetic bookmarks/session |
| Registered-root recovery and compatibility floors | Synthetic multi-root swaps, registry restoration, partial-permission failure/retry, corrupt/future snapshot rejection and missing legacy coverage | Actual journal-2 automatic update and interrupted recovery preserving every synthetic registered root; no successful rollback after partial restoration |

Do not publish intentionally corrupt previews to an active feed, change production trust
sources, disable TLS/sandboxing or add a health-signal bypass to obtain a negative result.
Unsafe or unavailable native failure exercises remain open. Local fixtures are useful
regressions, not evidence that the actual clean installed browser recovered.

Retain previous apps, profile snapshots, failed state and transaction journals until the
replacement and required recovery checks are healthy. Test removal only in the isolated
account, following the [removal guide](install-macos.md#remove-stainless). Forget update
authorization there before removing its app; never delete the build user's data or packages.

## Evidence to attach to the epic

Record check time and official upstream URLs, N/N+1 release URLs, product versions/builds,
full source and Chromium revisions, final DMG/ZIP hashes, profile schemas, hardware/OS/arch,
installation origin and whether this was a separate user or host. Redact personal paths,
account identifiers and all credentials.

For each action above, record pass/fail/not run, observed status/error, state before/after,
retained recovery resources and limitations. Distinguish:

- implemented source and synthetic HTTPS/filesystem checks;
- built/packaged/published bytes from exact committed source;
- downloaded and installed release on the build Mac/user;
- separate clean-client automatic N-to-N+1, real browser restart, state preservation and recovery.

Do not infer a higher category from a lower one. Keep Phase 1 and broader capability issues
open until their [release-evidence contract](phase-one.md#release-evidence) is actually met.
