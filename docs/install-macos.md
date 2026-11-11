# Installing and updating Stainless on macOS

**Unsigned private preview candidates are available. Phase 1 verification remains in progress.**
The local DMG install/eject/browsing and update-archive checks pass. A separate clean-Mac
installation and authorized automatic N-to-N+1 restart still need release evidence.
Use the [clean-user verification procedure](update-verification.md) to record that evidence
without substituting local payload fixtures or publisher credentials.

The five-patch baseline targets preview 4. **Google Safe Browsing, phishing/download reputation
and password-leak checks are removed without replacements.** Translation, cloud push and
other Google-backed features are unavailable. Read
[feature availability and replacement gaps](degoogling.md); older installed previews do not
have this configuration. Ordinary TLS checks and sandboxing must remain enabled.

## Install a private preview

The deployment target is Apple Silicon macOS 13.0 or newer; the locally verified OS is macOS
26.6. Do not treat the minimum deployment target as a tested support matrix. Actual tested OS
versions are recorded on each release. Intel/universal packages are not provided.

1. Sign in to GitHub with an account authorized for this private repository and open its
   [releases](https://github.com/mbianchidev/stainless/releases).
2. Download the matching `Stainless-VERSION-macos-arm64.dmg` and `SHA256SUMS`.
3. Check the DMG against the published hash:

   ```sh
   shasum -a 256 Stainless-VERSION-macos-arm64.dmg
   ```

4. Open the image and copy `Stainless.app` to `/Applications`. `~/Applications` is also
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

## Automatic private updates

Ordinary browsing is account-free. Access to this private repository's updates is optional
and requires initial authorization:

1. Open the **Stainless Updates** menu-bar item.
2. Choose **Authorize Private Updates**.
3. Choose **Generate token on GitHub**. The link pre-fills the token name/description,
   repository owner, **Contents: Read-only** permission and a 30-day expiry.
4. Review the owner on GitHub and choose **Only select repositories > stainless**.
   GitHub's documented template URL parameters do not preselect an individual repository;
   this last selection remains explicit.
5. Generate the token and paste it into the still-open Stainless dialog, then choose
   **Save in Keychain**. Never use a publisher/release-write token or paste a token into chat.

The button opens GitHub only when clicked. It does not mint a token, approve permissions,
read your GitHub session or store anything before you explicitly save the token.
Template fields follow [GitHub's documented URL parameters](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens#pre-filling-fine-grained-personal-access-token-details-using-url-parameters);
review the final permissions and repository selection before creation.

The credential is saved in the macOS Keychain, not preferences, feed metadata, process
arguments or logs. **Forget Update Authorization** removes it. Rebuilt unsigned updater
binaries can cause macOS to request Keychain access again; do not grant access to unexpected
applications. GitHub organization/SSO policies can also require separate token authorization.

After authorization, checks run on browser startup and every four hours while the updater is
running. A matching newer preview is downloaded and staged automatically. The menu exposes
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

The [opt-in profile developer tool](profile-isolation.md) defines separate roots/process
ownership, explicit copy-only preview migration and restore into a new root. It is not
installed onboarding or whole-profile encryption. The updated native helper includes registered
roots and their compatibility metadata in journal-format-2 snapshot/rollback transactions.
This is implemented source, not a newly published preview or verified installed multi-root update.

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
