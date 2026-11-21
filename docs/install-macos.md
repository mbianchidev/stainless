# Installing and updating Stainless on macOS

**Unsigned preview candidates are available. Phase 1 verification remains in progress.**
The local DMG install/eject/browsing and update-archive checks pass. A separate clean-Mac
installation and authorized automatic N-to-N+1 restart still need release evidence.
Use the [clean-user verification procedure](update-verification.md) to record that evidence
without substituting local payload fixtures or publisher credentials.

The newest published preview is **0.1.0-preview.8** (Chromium 158.0.8127.37).
**Google Safe Browsing, phishing/download reputation and password-leak checks are removed.**
Published previews have no replacements; current source adds on-device
[threat lists and opt-in breach checks](threat-protection.md). Translation, cloud push and other Google-backed features are
unavailable. Read [feature availability and replacement gaps](degoogling.md); previews older
than preview.4 do not have this configuration. Ordinary TLS checks and sandboxing must remain
enabled.

## Local development candidate

Preview.8 is published on [Releases](https://github.com/mbianchidev/stainless/releases);
see [project status](status.md) for what it adds. The default branch is still versioned
0.1.0-preview.8, so a local package from it is a development build, not an update to an
installed copy. A newer preview needs a fresh version through the [release process](releasing.md).

Local preview.5 to preview.7 candidates were built and checked locally but never published.
Their retained local evidence follows.

### Preview.7 verification

**Verified local artifact, 2026-11-15:** clean source
`8292680ce3cbfc3de15cc975a088aac5932b0502`, Apple Silicon macOS 26.6,
Xcode/SDK 27.2, eight build jobs. The DMG is **269,364,077 bytes**, SHA-256
`110bcd45d9abb8408bfbd728dbe5d90163fcf4cb19f632f1f27537a01a84c39d`.
Unmounted image verification and all 20 paired output checksums pass.

The corrected browser visibly lists the same other local profiles as the avatar
menu and updates names, avatars, omission and removal live. The list is read-only;
switching/management remain native profile-menu actions. Private contexts do not
expose the roster, and stopping its subscription stops list updates. The actual
native menu displays **Stainless Services Settings**, not the old Google label,
without restoring Google-backed controls.

All **45 final native tests plus five restart PRE stages** pass, followed by a
fresh three-test Profiles repetition, with no retries. Real Rust release tools
and the full browser build perform native Apple stripping/signing with no
`rust-objcopy` warning; the regression fixture confirms removed local symbols,
unchanged execution, valid signing and preservation of rejected inputs.
No tests were removed.

Both staged and freshly extracted actual ZIP apps pass complete browser smoke,
arm64/version7/schema1 and deep strict ad-hoc signature checks. The real-payload
extraction/validation Rust test also passes. The prior twelve patches, earlier
usable caches and user-opened preview.6 installer are preserved unchanged.
Official Mac Stable/source VERSION still agree on **158.0.8127.37 /
`b8c2be851f62f6a394ec57955d7f31335d7cb6d4`** at
**2026-11-15T14:51:34Z**.

This image was not mounted, opened or installed automatically. Final Finder
appearance, installed/clean-Mac workflows, physical accessibility and authorized
updates remain separate gates. Local provenance is recorded under
`dist/.development-artifacts/`; no release was published.

### Preview.6 retained evidence

**Retained preview.6 artifact verified, 2026-11-15:** clean source
`a511f658fbdead6c11074f139998207f9ab51eb4`, Apple Silicon macOS 26.6,
Xcode/SDK 27.2, eight build jobs. The DMG is **269,355,473 bytes**, SHA-256
`680d190152726fb826e08cc602910f1347f597dff614eceeb842e213c08790a4`.
Unmounted image verification and all 20 paired output checksums pass.
The actual ZIP app has version/build 6, profile schema 1, arm64 architecture
and a valid deep/strict ad-hoc signature. Both the staged app and a fresh
extraction pass the full isolated browser smoke, including TLS, private/Safe Mode,
six sandboxed renderers and zero Google-service requests during bounded
idle/browsing. The real-payload extraction/validation Rust test also passes.

The initial staged-startup attempt exited before a ChromeDriver session.
Signing-only and metadata-only controls did not reproduce it; diagnostic
packaging and fresh-payload checks pass. No root cause or general startup
reliability claim is inferred from those later passes. Startup failures now
retain the owned mode and bounded driver diagnostics instead of losing them
with temporary files. Four consecutive-payload update tests still need an
actual prior package; no tests were removed.

The image was not mounted, opened in Finder or installed during generation.
Its staged Finder metadata/artwork are checked, but final visible Finder
appearance and installed workflows remain unverified. Official Mac Stable
and source VERSION still match Chromium 158.0.8127.37 /
`b8c2be851f62f6a394ec57955d7f31335d7cb6d4` at **2026-11-15T13:07:31Z**.
The earlier usable caches are preserved; the local artifact owner/source/hash
record is under `dist/.development-artifacts/`, outside publishable assets.

## Install a preview

New packages use the Stainless S icon for both the mounted disk and the DMG
file. For releases providing `Stainless-VERSION-macos-arm64.dmg.zip`, download
that archive and extract it with macOS Archive Utility to retain both icons.
The plain `.dmg` remains available, but a raw download discards its external
Finder icon metadata; its embedded mounted-volume icon still works.
Verify the selected `.dmg.zip` or `.dmg` against its own `SHA256SUMS` entry.
The additional download archive is separate from the existing automatic-update
`.zip`; the updater format and feed are unchanged.

The deployment target is Apple Silicon macOS 13.0 or newer; the locally verified OS is macOS
26.6. Do not treat the minimum deployment target as a tested support matrix. Actual tested OS
versions are recorded on each release. Intel/universal packages are not provided.

1. Open the repository's [releases](https://github.com/mbianchidev/stainless/releases).
   No GitHub account is required to download.
2. Download the matching `Stainless-VERSION-macos-arm64.dmg` (or `.dmg.zip`
   where available) and `SHA256SUMS`.
3. Check the selected file against its published hash, for example:

   ```sh
   # For the plain image:
   shasum -a 256 Stainless-VERSION-macos-arm64.dmg
   # Or, for the metadata-preserving download:
   shasum -a 256 Stainless-VERSION-macos-arm64.dmg.zip
   ```

4. If downloaded as `.dmg.zip`, extract it with Archive Utility first. Open the
   image and copy `Stainless.app` to `/Applications`. `~/Applications` is also
   supported and is useful when your account cannot replace apps in `/Applications`.
5. Eject the image and launch the installed copy. No checkout, compiler, Xcode, Rust, GitHub
   CLI, model, cloud account or developer toolchain is required for browsing.

The app and DMG have no Developer ID signature or Apple notarization. Verify the source
and release host before opening. If macOS only reports an unidentified developer or missing
notarization, prefer Apple's [per-app exception](https://support.apple.com/en-us/102445):
**System Settings > Privacy & Security > Open Anyway**.

For a verified/trusted unsigned Chromium app blocked specifically by download quarantine,
the bundle-scoped fallback is:

```sh
xattr -dr com.apple.quarantine "/Applications/Stainless.app"
```

Use the actual installed path if it differs. This does not authenticate the publisher or
repair damaged files. Never disable Gatekeeper globally, override malware/managed-device
warnings, or apply this to an untrusted download.

## Managed local profiles

Preview.8 adds **Stainless Updates > Managed Profiles...** to the existing native
menu-bar helper. **Preview.4 and older do not have this chooser or its recovery window.**
Preview.8 was checked with actual browser settings checks and offscreen native-helper
coverage, not installed native-workflow verification.

Launch Stainless normally, open that menu and select a
registered local profile or explicitly **Create Profile** with a name. Creation does not
import data or launch a browser. **Open Standard** opens the selected root's own local identity;
**Open Incognito** opens a new temporary root without copying that identity's data.
In current source, **Open Guest** needs no selected profile: it opens a separate
[memory-only Guest](profile-isolation.md#memory-only-guest) whose whole root lives on a hidden
RAM disk, blocks downloads and saving pages, and is erased when it closes.
The existing preview/browser remains separate and open. No account, update
authorization, network, checkout, compiler or developer CLI is required.

Use Tab/Shift-Tab and native picker arrow keys. Return opens Standard, or creates while editing
the name; Escape closes the chooser without closing its browsers. Downloads persist outside
Incognito, and a crash can leave private-session files. Separate roots/processes are not
whole-profile encryption or an OS boundary against malicious same-user processes.

An owned Standard root must be closed before reopening. Pending update/recovery, unsupported
app/helper capabilities and launch/cleanup failures are reported without opening the default
identity as a fallback. Finish or recover the preview update before creating/opening profiles.
The helper stays alive to supervise managed browsers and recovery copies; close browsers and
wait for operations before quitting it.
See [ownership, runtime authority and recovery limits](profile-isolation.md#native-chooser-unpublished-source).
Stronger modes, deletion and automatic migration are not offered.

## Automatic updates

Ordinary browsing is account-free. Updaters built from current source read this public
repository's releases anonymously, so no token or GitHub account is needed. An optional
read-only token only raises GitHub's anonymous API rate limit (60 requests per hour per IP
address) or reaches a private fork. **Published previews up to preview.8 still require a
token** until they update to a release built from current source.

To add a token (required for preview.8 and earlier, optional afterwards):

1. Open the **S icon** in the menu bar, headed **Stainless Updates** when opened.
   Current source shows a one-color S, without adjacent text, that follows the light or dark
   menu bar like other macOS menu-bar icons, including during checks, errors and recovery.
   Its tooltip and accessible name still identify
   **Stainless Updates** and expose status/error details. Missing or invalid bundled artwork
   fails visibly with `!` and a reinstall message instead of an invisible menu item.
   Existing installed previews keep their previous UI until updated.
2. Choose **Add Optional GitHub Token** (**Authorize Private Updates** in preview.8 and
   earlier).
3. Choose **Generate token on GitHub**. The link pre-fills the token name/description,
   repository owner, **Contents: Read-only** permission and a 30-day expiry.
4. Review the owner on GitHub and choose **Only select repositories > stainless**.
   GitHub's documented template URL parameters do not preselect an individual repository;
   this last selection remains explicit. If you are not the repository owner, set yourself
   as the resource owner and choose **Public repositories** (read-only) instead.
5. Generate the token and paste it into the still-open Stainless dialog, then choose
   **Save in Keychain**. Never use a publisher/release-write token or paste a token into chat.

The button opens GitHub only when clicked. It does not mint a token, approve permissions,
read your GitHub session or store anything before you explicitly save the token.
Template fields follow [GitHub's documented URL parameters](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens#pre-filling-fine-grained-personal-access-token-details-using-url-parameters);
review the final permissions and repository selection before creation.

The credential is saved in the macOS Keychain, not preferences, feed metadata, process
arguments or logs. **Remove GitHub Token** (**Forget Update Authorization** in preview.8 and
earlier) removes it; current-source updaters then continue anonymously. Rebuilt unsigned
updater binaries can cause macOS to request Keychain access again; do not grant access to
unexpected applications. GitHub organization/SSO policies can also require separate token
authorization. Anonymous checks that hit the rate limit or a private repository report that a
token would help; browsing is unaffected.

Checks run on browser startup and every four hours while the updater is
running (preview.8 and earlier only after authorization). A matching newer preview is downloaded and staged automatically. The menu exposes
current/staged/error status, details on hover, a manual check and **Restart to Update**.
Checks failing offline or with denied access never prevent browsing.
Previously verified staged updates remain available after relaunch even if the next check
is offline or denied. Installation failures remain visible in the menu without repeatedly
retrying on a timer; use **Restart to Update** to retry after fixing the reported problem.

The updater only accepts this repository's HTTPS API/release-asset sources, validates normal
certificates, limits redirects, matches version/channel/macOS/arm64/profile schema and verifies
the complete ZIP's size and SHA-256. It rejects malformed, corrupt, stale/downgrade and
incompatible-schema payloads without replacing the app. Download credentials are not forwarded
to cross-origin release-asset redirects.

**Restart to Update** requests normal browser quit. Resolve any download/unsaved-work prompt
yourself; the updater does not force-close active work. It will only replace files after every
Stainless browser instance for the current macOS user releases its installation lock. A staged update can also
install after a normal quit, ready for your next launch.

Preview update coordination and profile snapshots are per macOS user. Concurrent updates
to one shared app from multiple logged-in OS accounts are not a validated support scenario;
use separate `~/Applications` installations on shared Macs.

The updater is an unprivileged user process, not a root daemon. Installation errors, including
non-writable Applications folders, remain visible. It does not invoke `sudo` or silently
elevate. Use `~/Applications` or arrange appropriate app-folder permissions.

## Backups and recovery

Replacement uses an atomic macOS directory exchange: an interruption cannot create a gap
where neither app occupies the installation path. The previous app is retained beside the
installed app as `.Stainless.previous-TRANSACTION.app`.

The default supported profile is `~/Library/Application Support/Stainless`; caches are under
`~/Library/Caches/Stainless`. Chrome/Chromium data and keychain identities are distinct and
are never imported implicitly. Arbitrary custom `--user-data-dir` paths are not automatically
backed up; only the default preview and explicitly registered managed roots are considered.

The [managed-profile runtime and developer tool](profile-isolation.md) define separate roots/process
ownership, explicit copy-only preview migration and restore into a new root. The preview.8
chooser adds list/create/launch and explicit managed cold backup/clone-restore, not migration
onboarding or whole-profile encryption.
The preview.8 native helper includes registered
roots and their compatibility metadata in journal-format-2 snapshot/rollback transactions.
An installed multi-root update has not been verified yet.

In a compatible preview.5 package, use **Managed Profiles... > Backup / Restore...** before
quitting the launcher browser. Recovery keeps the helper open and defers its update
checks/installation until **Finish Recovery** or closing the idle recovery window.
Save work and quit every Stainless browser normally, optionally using **Request Normal Browser
Quit**. Resolve unsaved-work/download prompts yourself; no browser is force-closed. A remaining
browser, supervised session, owner/singleton lock or pending recovery still blocks cold copies.

Select the managed profile, review the sensitive-plaintext limits, approve the operation and
choose **Back Up Selected**. Review/select committed compatible backup metadata, enter a new
label, approve again and choose **Restore New Profile**. Incompatible, invalid and incomplete
copies stay visible but cannot authorize restore. File integrity and current browser compatibility
are rechecked before a new identity/root is registered. Originals, failed roots and snapshots are
retained; no existing profile is overwritten. Finish recovery before explicitly opening the
selected new profile. No app bundle or Keychain credentials are restored or moved.

Copy/verification errors remain visible. If an operation is interrupted or its result/durability
cannot be confirmed, refresh profiles/backups before retrying: a commit marker or new registry
entry may already exist. Never delete locks or retained copies to bypass the reported problem.
No account, update authorization or developer tools are needed. See the
[native recovery flow and limits](profile-isolation.md#native-backup-and-clone-restore-unpublished-source).

After every browser quits, replacement snapshots the default profile and every registered
managed data root under:

```text
~/Library/Application Support/Stainless Updater/backups/TRANSACTION/
  profile/                  Original preview, if present
  profiles/p-ID/            Each registered managed data root
  profiles.json             Verified inventories and captured registry/floors
  recovery.json             Failed registry and resumable rollback plan, when needed
```

Failure to snapshot any registered root blocks the whole installation. Managed caches,
unregistered/custom roots and Keychain items are not copied. Keep explicit backups and the
matching app when the installed helper predates journal format 2: older helpers cannot gain
managed snapshots from a newer payload. Legacy preview-only journals remain readable, but
automatic rollback with missing legacy managed coverage stops for explicit recovery.

After launch, the browser's main event loop must remain responsive long enough to write its
30-second startup marker. The updater keeps the previous app and snapshot even after success.
If a new launch exits before confirmation, it restores all captured data roots and registry
compatibility floors before restoring the prior app. Failed preview state is retained as
`.Stainless.failed-profile-TRANSACTION` beside the profile; managed failed state remains under
`Stainless Profiles/profiles/p-ID/failed-TRANSACTION/`.

An interrupted rollback stays `recovering`, not healthy/rolled back, until every recorded
root, registry and app step completes. A later startup marker cannot cancel that recovery.
After a permission/storage failure, close browsers, repair the reported problem and retry
with the retained updater. Unexpected registry changes or writes to partially restored roots
stop recovery rather than overwriting them. Preserve the journal and any retained `r-ID/`
recovery copies; do not launch browsers manually during unfinished rollback.

A hung process must be closed by the user before recovery can acquire the exclusive lock.
The updater never replaces an app that is still running. If the computer loses power or the
updater is explicitly quit, backups/journal remain available, but automatic recovery may need
the retained updater or a manual repair. A failed build is not repeatedly downloaded unattended;
use a newer preview or deliberate manual recovery.

For manual repair, quit every Stainless browser/updater, preserve both current and failed
profiles, and restore the matching previous app **together with every captured data root and
its registry/compatibility metadata**.
Do not simply install an older browser over a profile it may not understand. When uncertain,
keep the profile untouched and install the latest working DMG instead.

If staging is corrupt, use **Check for Updates** to retry. Keep copies of releases/build
manifests and do not delete the previous app or snapshots until you have a healthy replacement.
Backups contain sensitive plaintext Chromium profile data and are not an encrypted vault.

## Trust and current limitations

HTTPS plus release-host/account authorization is the initial update trust boundary.
SHA-256 detects corruption; it does not independently authenticate the publisher, defeat
a compromised release host/account, or provide replay-resistant signed metadata. Local ad-hoc
signatures are executable-integrity checks, not publisher identity.

No signing key, notarization credential, telemetry or privileged always-on service is needed.
Authenticated metadata/transparency and stronger rollback protection are later work.
The packaged browser and archive integrity checks pass locally. Real private authorization
and the complete automatic N-to-N+1/recovery path remain unverified until the
[release evidence](phase-one.md#release-evidence) is recorded.

## Remove Stainless

Forget update authorization before removing the app. Quit Stainless and the updater, then
move the installed app to Trash. Keep profile/backups if you want to reinstall.

If deliberately removing local data too, review and remove only the Stainless-specific
Application Support/cache/updater directories above, retained previous-app bundles and
failed-profile directories. Do not remove Chrome/Chromium folders or unrelated cache roots.
The separate `Stainless Safe Storage` Keychain item can be removed after you no longer need
encrypted cookies/passwords from the retained profile.
