# Profile ownership, migration and update recovery

This is bounded groundwork for [#4](https://github.com/mbianchidev/stainless/issues/4) and the
first managed Standard/Incognito slice of #7/#8. It adds the opt-in Rust developer tool,
registered-root updater recovery and an account-free AppKit chooser in the existing native
helper, including explicit cold backup, clone-restore and user-selected startup Safe Mode.
The chooser/recovery UI is
**unpublished source integration**, not a verified installed-browser
experience or a new release. Existing previews continue to open their original root.
It does not complete #4, Phase 1's [release evidence](phase-one.md#release-evidence), or
Phase 2's protected-data isolation gate.

**Separate roots and browser processes are not whole-profile encryption or an OS security
boundary between same-user browser processes.** History, bookmarks, preferences, sessions and
backups remain ordinary Chromium data. The current trusted browser/maintenance processes can
read those files. A metadata-only registry is not a sandboxed metadata-only coordinator.

## Versioned ownership

Registry format 1, metadata protocol 1 and manual backup format 1 are independent versions.
New native updater transactions use journal format 2, profile-snapshot format 1 and recovery-plan
format 1. Older installed helpers do not gain that coverage until replaced.
The product's existing `profile_schema` stays **1**. No encrypted-store migration, profile-data
schema bump, key rotation or downgrade conversion is introduced.
Unknown versions/fields, duplicate IDs, invalid IDs and unsupported data schemas fail closed;
the tool does not rewrite, reset or silently reconstruct an incompatible/missing registry.
The registry supports up to 256 profiles and 1 MiB of serialized metadata. Each backup/update
snapshot manifest and recovery plan is limited to 16 MiB; the updater snapshot limit covers
all roots combined. Write limits include pretty formatting and the final newline, so a
successful commit remains readable under the same byte limit.

All paths are derived beneath the current macOS user's home, never from labels or IPC paths:

```text
~/Library/Application Support/
  Stainless/                              Original preview user-data root, unchanged
  Stainless Updater/install.lock          Existing installation-wide browser/update lock
  Stainless Updater/backups/TRANSACTION/
    profiles.json                         Snapshot commit marker, registry and all file inventories
    profile/                              Original preview's cold copy, if it existed
    profiles/p-ID/                        Each registered managed root's cold copy
    recovery.json                         Rollback plan, directory identities and failed registry
  Stainless Profiles/
    registry.json                         Version, IDs, labels, schema, browser compatibility floors
    registry.lock                         Short-lived registry serialization
    profiles/p-ID/
      owner.lock                          Exclusive browser/maintenance lease for this root
      data/                               One complete Chromium user-data root
      cache/                              This profile's explicit disk-cache root; not backed up
      failed-TRANSACTION/                 Failed data retained after automatic rollback
    backups/b-ID/
      snapshot.json                       Commit marker, source metadata, SHA-256 file inventory
      data/                               Sensitive, plaintext cold copy
```

IDs are generated opaque names, not user-supplied paths. Managed directories are owner-only
`0700`; new metadata, locks and copied files are `0600`. Symlinked directories, aliased lock
files, hardlinked source files and unexpected symlinks/special files are rejected.
These checks prevent accidental aliasing and unsafe copies, not a malicious same-user process
racing filesystem operations.

The registry contains only IDs, user-entered labels, data schema and optional last-browser
product version/build/channel/Chromium version. It contains no URLs, tab/workspace contents,
history, bookmarks, cookies, credentials, vault data, key handles or arbitrary filesystem paths.
Labels are plaintext metadata too; use neutral names when identity labels are sensitive.
Backup inventories are separate from the registry and can reveal sensitive filenames.

Here, a managed profile means a **whole Chromium user-data root**. Existing Chromium
`Default`/`Profile N` subprofiles are copied together into one ownership group; they are not
silently split or advertised as separate security boundaries. The chooser selects these roots,
not Chromium subprofiles. Spaces and workspaces are not redefined by this slice.

## Process and key boundaries

Current architecture, version 4:

```mermaid
flowchart LR
  Native["Native AppKit managed-profile chooser"] -->|"typed local requests, bundled helper only"| Tool["Trusted Rust profile runtime / developer tool"]
  Tool -->|"metadata protocol 1: list/create"| Registry["Registry 1: metadata only"]
  Tool -->|"owned foreground child + per-root lease"| BrowserA["Browser process A"]
  Tool -->|"owned foreground child + per-root lease"| BrowserB["Browser process B"]
  BrowserA --> DataA["Root A + cache A"]
  BrowserB --> DataB["Root B + cache B"]
  BrowserA --> SandboxA["Inherited sandboxed renderer / utility children A"]
  BrowserB --> SandboxB["Inherited sandboxed renderer / utility children B"]
  BrowserA --> SafeStorage["Inherited per-user Stainless Safe Storage Keychain item"]
  BrowserB --> SafeStorage
  Tool -->|"cold file access under exclusive installation lock"| Backups["Backup 1: plaintext copies + inventory"]
  Native -->|"explicit recovery consent, separate profile-recovery command"| Tool
  Updater["Existing native/Rust updater"] --> Legacy["Original preview root + updater snapshots"]
  Updater -->|"cold snapshot / rollback"| DataA
  Updater -->|"cold snapshot / rollback"| DataB
  Updater -->|"snapshot / restore compatibility metadata"| Registry
  Updater --> Token["Separate update-token Keychain item"]
  BrowserA --> Lock["Installation lock: browsers shared, replacement/maintenance exclusive"]
  BrowserB --> Lock
  Tool --> Lock
  Updater --> Lock
```

Standard launches supply derived `--user-data-dir` and `--disk-cache-dir` switches plus the
fixed managed-mode selector. No arbitrary browser arguments are accepted.
It owns the actual child handle and holds its profile lease and shared installation lock
until that child exits. No PID from a registry/request is trusted, attached to or killed.
Different roots can run concurrently; a second owner of the same root is rejected.
The app identity and compatibility floor are read/recorded while holding the installation
lock, before the browser can change storage. Failed spawn/launch retains that conservative floor.
Lower product builds/versions or Chromium versions cannot open the root, even if schema 1 matches.
The on-disk Chromium `Last Version` also prevents an older engine from opening externally
advanced data.

The tool waits in the foreground and never force-closes unsaved browser work. If its supervisor
dies, Chromium's existing installation lock and singleton remain additional protections.
A remaining `SingletonLock` is treated as ambiguous ownership, even if a PID appears stale.
Inspect it with every browser closed; the tool never deletes it or guesses which process to kill.
Actual Chromium singleton cleanup is inherited from the pinned POSIX/macOS implementation.

## Managed browsing modes

`profiles modes` reports the policy matrix without opening a registry or reading browsing
data. The native chooser and developer `run`/`open` commands share the production foreground
launcher, not a settings mock or a replacement shell. Mode selection is immutable for that launch.

| Mode | Storage and identity | History and extensions | Network and downloads |
| --- | --- | --- | --- |
| Standard | The selected managed root and its existing local identity/passwords | Chromium's history preferences and user-controlled extensions | Ordinary direct networking; user-directed files persist |
| Managed Incognito | A new, owner-only temporary root/cache for each launch; no copied cookies, passwords, bookmarks, preferences or other profile files | Forced Chromium Incognito; no browsing history; extensions and password saving disabled | Ordinary direct networking, not anonymity; downloaded files persist outside the temporary root |
| Guest | Unavailable: memory-only storage, restricted downloads and guaranteed teardown are missing | Unavailable | Unavailable |
| Privacy Container | Unavailable: verified storage/network partitioning and privacy enforcement are missing | Unavailable | Unavailable |
| Developer | Unavailable: workspace trust and scoped developer capabilities are missing | Unavailable | Unavailable |
| Agent Sandbox | Unavailable: disposable isolation, approvals and resource ceilings are missing | Unavailable | Unavailable |
| Tor Proxy / Hardened Onion | Unavailable: verified routing, leak prevention and failure isolation are missing | Unavailable | Unavailable |

AI, agents and the custom vault are unavailable in every mode. Standard inherits Chromium's
ordinary behavior; this does not implement #7's future tracker-blocking/partitioning guarantees.
Guest is also unavailable in the new browser's native picker and command-line startup.

Managed Incognito requires an app declaring `StainlessManagedModesVersion = 1`; older apps
are rejected rather than silently ignoring enforcement. The downstream browser applies
immutable forced-Incognito, no-add-profile, no-password-saving, no-extension and no-background-mode
policies to that session. Standard remains account-free and can use local profiles and passwords.
These are source changes, not capabilities retroactively added to published previews.

The private root lives under `profiles/p-ID/incognito/s-ID/{data,cache}`. It holds its own
fresh browser process and the shared installation lock, so it can coexist with the selected
Standard profile but blocks replacement/maintenance until it exits. It neither opens the
persistent data root nor advances that root's compatibility floor. Cold profile backups and
update snapshots cover persistent data, not these temporary sessions.

After an observed normal or unsuccessful browser exit, the launcher explicitly removes that
session directory and reports cleanup failures. If child ownership is uncertain, it retains
the directory instead of deleting a potentially live browser's files. Killing the supervisor,
a crash or a power loss can leave temporary files; there is no memory-only, secure-erasure or
guaranteed-teardown claim. With the browser closed, remove only the exact retained session
directory identified by diagnostics. Downloads, OS caches and external observers are separate.

`open APP PROFILE_ID MODE URL` accepts only an explicitly supplied HTTP(S) URL without embedded
username/password credentials. No live tab, referrer, form state, cookies, credentials, storage
or artifacts are read or copied. Repeating the operation creates a fresh private session.
URLs can themselves contain sensitive query/fragment values; review them before transferring.
A Standard destination still uses **its own** existing identity; close it first if already
owned. No in-place tab conversion is provided.

Chromium's native New Incognito Window inside a Standard launch remains inherited behavior:
it can share that root's preferences, bookmarks and explicitly allowed extension/password
facilities. It is not the fresh-root managed launch described here. Neither path is an
OS-enforced boundary against a malicious same-user process.

The current [keyboard shortcut editor](build-macos.md#profile-keyboard-shortcuts) uses an
unsynced Chromium **profile preference**, not this outer managed-identity registry.
Normal-profile changes persist locally. Inherited Incognito initially reads the same
identity's normal bindings; private edits and resets stay in Chromium's in-memory preference
overlay and disappear when its last private window closes. Fresh-root managed Incognito
starts with defaults, without copying another identity's stores. Temporary startup Safe Mode
ignores saved shortcut overrides and makes the editor read-only without deleting them.
These source changes do not publish an installed feature or add profile encryption.

## Native chooser (unpublished source)

In the local preview.5 candidate, open the existing **Stainless Updates** menu-bar
item and choose **Managed Profiles...**. The original preview browser stays open and keeps
its own browsing state. This is an optional entry point, not replacement first-run onboarding
or an automatic switch of the app's default identity. Older published helpers do not have it.

- Select a registered label/ID, or enter a name and explicitly choose **Create Profile**.
  Creation makes an empty local root and selects it; it does not import data or launch a browser.
- **Open Standard** uses only that root's existing local identity. An already-owned root is an
  actionable error: close its browser before retrying. It never falls back to the preview.
- **Open Incognito** creates a fresh temporary root/process without copying the selected root's
  cookies, passwords, bookmarks, preferences or other stores. It can coexist with Standard.
  Downloads still persist; crash/power-loss cleanup is not guaranteed.
- **Open Safe Mode** temporarily opens the selected Standard identity without risky startup
  facilities. See [startup restrictions and the normal-return route](#managed-startup-safe-mode).
- Tab/Shift-Tab navigate native controls; the picker uses native arrow-key selection. Return
  opens Standard, or creates a profile while editing its name. Escape closes the chooser.
  Errors are selectable, scrollable and announced through AppKit accessibility.

No account, update authorization or network is required for these operations. There are no
Google sign-in/sync controls, cloud importers, Guest/future-mode buttons, deletion or automatic
migration in this surface. Explicit supported browser-file import/export remains separate
and repeatable as described in [feature availability](degoogling.md#repeatable-explicit-import-and-export).

Closing the chooser does not close its browsers. A foreground Rust helper per launch retains
the actual child handle, per-root ownership where applicable and the shared installation lock
until that browser exits. The native helper refuses ordinary quit while those sessions or
profile operations remain, including backup/restore. A disconnected UI/output pipe does not
make the supervisor release ownership early. A start acknowledgement confirms process creation,
**not browser health**.
Launch/exit/cleanup failures are reported without claiming success or switching identity.
Failures remain in the chooser and mark its menu entry **action needed**, including when
its window was closed. An unrelated successful browser exit cannot erase them. Refresh or
another explicit profile operation acknowledges the previous error; ordinary helper quit
remains available after its browsers/operations finish.

Creation/launch reject pending or unknown update/recovery transactions and concurrent installation
replacement. Existing registry metadata can still be listed while recovery is pending; first
registry initialization is blocked, so opening the chooser cannot change a preview-only
transaction's captured registry state. Finish/recover the preview update before retrying.
Do not delete locks, reset compatibility floors or manually launch a root during recovery.

The UI selects only the current installed app's
`Contents/Helpers/Stainless Updater.app/Contents/Resources/stainless-update`. It rechecks the
installed manifests on each operation, requires native chooser protocol/capability **1** and
browser managed-modes capability **1**, and rejects missing/unsupported/escaping helpers.
There is no checkout, compiler, Rust, developer `xtask`, shell-command, alternate executable
or cached-updater fallback on this runtime path. The native UI and Rust helper hashes plus
`profile_chooser_version`, `profile_recovery_version`, `profile_safe_mode_version` and
`managed_safe_mode_version` are recorded by the build; the helper
capabilities are packaged in its `Info.plist`. Package assets include this guide.

Native launch authority is a **separate local operator command**, not an extension of the
metadata endpoint: `profile-launch` accepts one bounded protocol-1 JSON request containing
only `profile_id`, `mode` (`standard` or `incognito`) and an optional `safe_mode` boolean
(default `false`, Standard only). The app path is bound by the native
helper to its containing app; the Rust invocation verifies its own bundled executable while
holding the installation lock. No renderer supplies an app/data/cache path, URL, browser
argument, PID or secret handle. This is trusted native authority, not an exposed IPC/network
service, sandboxed metadata coordinator or protection from malicious same-user code.

## Managed startup Safe Mode

This is **unpublished source**, not a capability added to the preserved preview.5 candidate
or older installed helpers. It is a temporary startup overlay on **Standard**, not a new
browsing mode, Guest/Incognito conversion, anonymity or encryption.

1. Select the managed root whose normal startup fails. Quit that root's browser normally
   before retrying; other roots may remain open. **Open Safe Mode** uses the same persistent
   identity, compatibility floor, installation lock and exclusive per-root lease as Standard.
   Pending updates/rollback, a concurrent owner or ambiguous singleton still block it.
2. Safe Mode skips saved user-extension manifests and extension maintenance, disables user
   extension execution and Chromium background application mode, and opens without automatic
   previous-session, startup-URL or pinned-tab restoration. Bundled browser components such as
   PDF remain available. Website workers/timers are not globally disabled.
   Saved startup/extension preferences, pinned-tab data, session/tab/app journals, session
   cookies/storage and prior exit markers are retained rather than reset for this launch.
   Session writers and their automatic cleanup are suspended; **Safe Mode tabs/pin changes
   are not saved for restoration**.
3. Browse using the same local identity, history, bookmarks and passwords. The native chooser
   shows the temporary restrictions; the browser has a persistent Safe Mode notice and
   temporary Stainless policies in `chrome://policy`. Ordinary browsing and explicit user
   changes can still write data: this is not an immutable profile copy. Use the existing
   **Backup / Restore...** entry point first when you need a cold snapshot. It still requires
   all browsers to close normally, explicit consent and the existing recovery/update boundary.
4. Save work and quit the Safe Mode browser normally. The chooser offers **Open Normally**
   for that identity and keeps the helper available even if its window was closed.
   Choose it explicitly to return to normal startup; no restrictions are written to the
   registry or automatically carried into that normal launch. Existing session data are again
   available to Chromium's normal restoration behavior.

Both `StainlessManagedSafeModeVersion` in the browser and `StainlessProfileSafeModeVersion`
in its bundled helper must be **1**, in addition to chooser/modes protocol 1. They are checked
before profile preparation; missing/newer capabilities fail visibly without a normal/default
fallback. A safe launch uses only the fixed `--stainless-startup-safe-mode` selector alongside
the production derived Standard root/cache arguments. No arbitrary flags, paths or URLs are
added to native launch authority:

```json
{"protocol_version":1,"profile_id":"p-IDENTIFIER000000","mode":"standard","safe_mode":true}
```

Safe start/exit replies also carry `safe_mode: true`; a helper that fails to acknowledge the
requested startup restrictions is an error, not silently accepted compatibility.
Process creation or exit is **not comprehensive profile health or successful recovery**.
Failure diagnostics retain recovery access and never force-close work, clear singleton files,
rewind floors, overwrite originals or delete snapshots.

There is no automatic crash counter/threshold, unattended reset, new backup system or promise
to repair every corrupt store. Full crash-safe journals, selective restoration, onboarding,
macros, custom rules, AI, agents and workspace automation remain unimplemented/open in #8
and their owners; no nonfunctional disable switches are offered for them.

## Native backup and clone-restore (unpublished source)

In the compatible source-built helper, choose **Managed Profiles... > Backup / Restore...**.
Opening recovery explicitly defers this helper's update checks, installation and restart;
it does not close browsers, read browsing stores, copy data or authorize a restore.
The existing chooser cannot create or launch another profile while this recovery window is
active. No update token, account, network, checkout or compiler is required.

1. Save work and quit every Stainless browser normally. **Request Normal Browser Quit**
   uses the existing native normal-quit control; resolve download/unsaved-work prompts yourself.
   No process is force-closed, and no registry/request PID is trusted or killed.
   The helper/window remains open after the launcher browser quits. Reported browser absence
   is only a UI hint: supervised sessions must finish and each copy must acquire the real
   installation lock exclusively. An unreported browser, owner lease, singleton or pending
   recovery still blocks the operation with an actionable error.
2. Select the managed label/ID to back up, review the plaintext/recovery limits, check the
   approval box and choose **Back Up Selected**. The production verified cold-copy path writes
   `snapshot.json` last. It excludes disk caches and never changes the original data root.
   Approval resets after an operation or a different selection; opening recovery alone is not consent.
3. Review a backup's source label/ID, schema, captured product/build/Chromium compatibility
   floor and commit-manifest modification time. That filesystem timestamp is advisory, not
   authenticated creation time. **More Backups** reads the next bounded page, without pruning.
   Listing reads metadata, not browsing stores or the file inventory's referenced files, and
   returns no inventories, hashes, credentials or store contents. Incomplete/invalid copies and
   newer incompatible snapshots remain visible but cannot enable restore. **Compatible**
   means metadata-compatible, not that content verification has already passed.
4. Select a committed compatible backup, enter a **new recovered profile name**, approve the
   operation and choose **Restore New Profile**. The runtime rechecks format, complete inventory,
   current app compatibility and pending recovery under the installation/registry locks before
   copying and registering a new opaque identity/root. Originals, failed roots, registry entries,
   manual backups and updater snapshots remain available. No existing root is overwritten or
   compatibility floor rewound. A restored profile is selected, **not automatically launched**.
5. Choose **Finish Recovery** or close the idle recovery window to release update deferral;
   then explicitly open the selected new profile when ready. Escape finishes idle recovery.
   A running copy prevents window/helper quit until it finishes. Tab/Shift-Tab and native
   picker keys reach every control; errors remain selectable and are announced through AppKit.

Restore does **not** restore an application bundle or move/export Keychain credentials.
Keep the matching prior app yourself. These are sensitive plaintext, corruption-checked copies,
not authenticated encrypted backups or an OS-enforced same-user isolation boundary.
They do not promise password/cookie decryption on another OS account or Mac.

Copies and verification run off the AppKit thread. Missing/unsupported helpers, live ownership,
pending/unknown updater transactions, incompatible/corrupt copies and permission/storage errors
are failures, not successful empty results. Keep incomplete/orphaned directories for explicit
review. If acknowledgement, commit or final directory sync fails, a backup marker or new registry
entry may already exist: **Refresh Profiles / Backups** before retrying rather than blindly
creating another copy. Existing browsing roots and snapshots are never removed to repair an error.

Recovery uses a separate bundled `profile-recovery` command with helper capability
`StainlessProfileRecoveryVersion = 1`, not new operations on `profile-metadata`:

```json
{"protocol_version":1,"request":{"operation":"list"}}
{"protocol_version":1,"request":{"operation":"backup","profile_id":"p-IDENTIFIER000000","confirmed":true}}
{"protocol_version":1,"request":{"operation":"restore","backup_id":"b-IDENTIFIER000000","label":"Recovered copy","confirmed":true}}
```

Use the actual generated 18-character IDs, not these illustrative identifiers. Listing accepts
an optional `after` backup-ID cursor and returns up to 64 records plus `next_after`.
Unknown fields/versions/operations, absent/false confirmation and arbitrary paths/arguments
are rejected. App/helper identity is revalidated under the appropriate installation lock;
the same production backup/restore implementation serves developer and native callers.
This trusted local operator command is not an exposed renderer, IPC or network capability.
The local preview.5 browser builds and passes isolated settings/mode smoke. Native chooser and
recovery checks remain offscreen synthetic coverage; actual installed UI/recovery verification
is still outstanding. This explicit Chromium 158 experiment does not resolve the
[Stable publication gate](build-macos.md#maintaining-chromium).

## Key/data owners and deferred protection

| Key/data owner | Current boundary and deferred work |
| --- | --- |
| Each browser process | Owns one managed root and its renderer/utility state. Existing Chromium sandbox/site isolation/TLS stay enabled. Same-user browser processes are not denied each other's files by a new sandbox. |
| Chromium Safe Storage | Uses the existing `Stainless Safe Storage` / `Stainless` service/account. This is inherited protection for selected Chromium secrets, not separate profile keys or encryption of every store. |
| Updater | Trusted file-level access to the preview and registered roots while browsers are closed; not metadata-only authority. Its GitHub credential remains in `dev.stainless.browser.preview-updates` / `github-read-token`, separate from browsing secrets. The profile crate/tool has no credential-reading, network or Keychain dependency. |
| Future vault worker | Not implemented. It must own per-profile key generation/unlock/rotation, minimize/zeroize key bytes and exclude them from dumps. Only session-bound, profile-scoped, purpose-limited, revocable handles may cross its authenticated IPC boundary; never raw keys. |
| Future shell/coordinator | Must run with metadata-only authority and no decrypted-store/vault API or shared extension/storage process state. Separate sandboxed workers and versioned Mojo are required before claiming that enforcement. The local tool is not that service. |

No whole-profile-encryption algorithm, encrypted database format or recoverable key-export
scheme is selected here. The future migration must explicitly identify plaintext legacy data,
create/verify encrypted stores and backups, define key-loss/rotation behavior, and test that
the coordinator and other profiles cannot read plaintext. Existing OS administrators/kernel
compromise are out of scope; malicious sites/extensions/tools/models and compromised same-user
browser/maintenance processes remain threats for those later boundaries.

## Metadata request contract

The only implemented request operations are `list` and `create`:

```json
{"protocol_version":1,"request":{"operation":"list"}}
```

```json
{"protocol_version":1,"request":{"operation":"create","label":"Work"}}
```

Responses have `protocol_version`, `registry_version` and `profiles`; create returns the new
profile's metadata, list returns all registered metadata. Requests are limited to 16 KiB.
Unknown operations/fields/versions, paths, browser arguments, browsing-data and key requests
are rejected, rather than forwarded to a browser or another service.

This is a typed one-request stdin contract for future adaptation, **not an exposed IPC
listener**, authenticated transport or Mojo implementation. Launch, backup, import and restore
are separate, explicit local operator commands, not capabilities on the metadata endpoint.
Before any service is exposed, bind the OS peer identity and profile/session capability to
the connection, validate version/size at the receiving worker, deny ambient file/key authority,
and test confused-deputy, cross-profile handle reuse, disconnect and revocation behavior.
No renderer, extension, agent or model gets this operator CLI as an ambient tool. The native
chooser uses the same metadata requests through its separate bundled `profile-metadata`
invocation; only its explicit `profile-launch` invocation has launch authority. Backup catalog,
cold backup and clone-restore belong to the separate `profile-recovery` operator command.

## Preview backup, migration and restore rules

1. Quit every Stainless browser normally. Finish or recover a pending updater transaction.
   Import, backup and restore require the existing installation lock exclusively; creation and launch also
   refuses pending/unknown updater transactions, so it cannot advance a managed root during
   automatic failed-launch rollback. No operation requests administrator access or force-quit.
2. Explicit preview import requires the matching `Stainless.app`, product schema 1 and its
   exact Chromium `Last Version`. The source is only `~/Library/Application Support/Stainless`;
   Chrome/Chromium, custom paths and other OS accounts are not imported.
3. Inventory the closed source, copy it into a unique backup, verify all files and recheck the
   unchanged source. Persist/sync `snapshot.json` last as the backup commit marker. Only root
   runtime artifacts `SingletonLock`, `SingletonCookie`, `SingletonSocket`,
   `RunningChromeVersion` and `DevToolsActivePort` are omitted; a present singleton lock already
   blocks the operation. Other unexpected links/files are errors, not silently skipped.
4. Copy the frozen backup into a new managed root and verify its inventory again. Atomically
   persist/sync the registry last. The original preview and backup are never moved, overwritten
   or deleted, and migration never launches a browser automatically.
5. Managed backups require the recorded browser identity to match the root's version.
   Restore checks the backup format/schema/inventory and copies into a **new ID/root** with its
   captured compatibility floor. The current/failed root, registry entries and backup remain
   available. An older app is allowed only on a compatible older snapshot, never by rewinding
   the compatibility floor on the live root.

Missing commit markers, corrupt/incomplete copies, incompatible formats, disk/permission
failures and source changes produce errors without replacing original data. Unregistered
profile directories and incomplete backups are retained with their paths in the error.
The explicit profile commands do not adopt orphans, prune backups or overwrite an existing
profile on restore. Automatic update rollback has the separate transactional rules below.
After a durability error, inspect the registry/commit marker before retrying: an atomic rename
may already have succeeded even if a later directory sync failed.

Keep the matching prior app and its release/build manifests yourself; profile backups do not
contain an application bundle or export Keychain items. Restore is same-account data recovery,
not a promise that cookies/passwords can be decrypted on another account/Mac. External
extension/download resources referenced by Chromium preferences are not copied from arbitrary
host paths. These backups contain sensitive plaintext, not an encrypted vault.

## Native multi-root update recovery

The updater acquires the exclusive installation lock, then the registry lock and every
registered root's owner lease. It snapshots the preview, every registered managed data root
and their exact registry metadata before replacing the app. It uses the same verified
cold-copy, no-follow and compatibility helpers as manual backups. An unreadable, missing,
aliased, owned, unpaired or incompatible registered root blocks the **entire** installation;
it is never silently skipped. A preview-only install does not create a managed registry.
Custom/unregistered roots and managed disk caches are not included.

The old app must be compatible with every captured root and the candidate cannot lower its
product/Chromium floor. `profiles.json` is committed last, before journal format 2 authorizes
app exchange. Creation, launch, import, backup and manual restore refuse pending or unknown
transactions; metadata listing remains available. Managed launches still use `--user-data-dir`
and do not start an independent update/health helper. Confirm the default preview's update
before opening them.

On failed startup, recovery verifies every snapshot and prepares every replacement before
committing `rolling_back`. Its durable plan records the original/prepared directory device
and inode identities, so an interrupted exchange cannot be replayed backwards. Each failed
root remains available: the preview beside its original path as
`.Stainless.failed-profile-TRANSACTION`, and each managed root under
`profiles/p-ID/failed-TRANSACTION/`. The plan retains the failed registry, including any
compatibility floor advanced to this candidate.

Only after all data roots are restored does recovery atomically restore their captured
registry/floors, exchange the app back and commit `rolled_back`. A late startup marker cannot
override `rolling_back`. Errors leave the journal nonterminal and the new app in place until
data/metadata restoration completes; an interruption after app exchange must still reconcile
the recorded roots. Partial restoration is not reported as successful rollback. Retry after
closing browsers and repairing the reported permission/storage problem. Unexpected registry
entries, changed restored data or displaced directory identities require explicit recovery,
not overwriting newer work. Interrupted preparation may retain owner-only `r-ID/` copies
beside the affected root; keep them while recovery is pending.

**First-upgrade limitation:** a transaction prepared by an older installed updater contains
only the preview snapshot. New code can read that legacy journal and convert preview-only
rollback to the resumable plan, but cannot invent historical managed snapshots. With
registered roots, missing legacy coverage blocks automatic rollback rather than omitting
them, even if an old helper already exchanged the app back. Before that first transition,
keep explicit managed backups and the matching prior app. A successful legacy startup does
not retroactively prove managed-root recovery.

This is source-level recovery integration, not a published browser or clean-host update
demonstration. Snapshots/failed roots remain plaintext, do not export Keychain items and are
not authenticated against same-user tampering.

## Developer usage and evidence

```sh
./tools/stainless profiles help
./tools/stainless profiles modes
./tools/stainless profiles list
./tools/stainless profiles create "Work"
./tools/stainless profiles import-preview /Applications/Stainless.app "Imported preview"
./tools/stainless profiles run /Applications/Stainless.app p-IDENTIFIER
./tools/stainless profiles run /Applications/Stainless.app p-IDENTIFIER incognito
./tools/stainless profiles safe-mode /Applications/Stainless.app p-IDENTIFIER
./tools/stainless profiles open /Applications/Stainless.app p-IDENTIFIER incognito https://example.com/
./tools/stainless profiles backup p-IDENTIFIER
./tools/stainless profiles restore b-IDENTIFIER "Recovered copy"
printf '%s\n' '{"protocol_version":1,"request":{"operation":"list"}}' |
  ./tools/stainless profiles metadata
```

Replace example IDs with the full generated IDs returned by the tool. These commands do not
need full Xcode, a clean Mac, updater credentials or a Chromium fetch/build. They do need the
repository's pinned Rust toolchain; installed browsers do not gain a compiler dependency.

| Risk | Implemented protection and synthetic evidence | Residual/release boundary |
| --- | --- | --- |
| Wrong-root routing or concurrent ownership | Two synthetic child processes receive distinct roots/caches; duplicate ownership and installation replacement are blocked until exit. | Not real-browser sandbox enforcement or proof of protected-data non-readability. |
| Live/inconsistent migration | Shared updater/browser lock, singleton refusal, pending-transaction checks, verified cold copies. | Unknown/manual browser builds must not bypass the inherited installation lock. |
| Data loss or incompatible restore | Original and failed roots retained; clone restore, hashes, commit markers, schema/version floors, interrupted-copy/future-format rejection. | Real bookmark/session/native restart and clean-host recovery need candidate-specific evidence. |
| Partial automatic rollback | Multi-root snapshot rejection, per-root/app exchange interruption, durable registry/floor restoration, permission-repair retry and legacy-coverage refusal. | A previously published helper cannot snapshot managed roots retroactively; real installed multi-root recovery remains an open release gate. |
| Filesystem/metadata substitution | Strict IDs/fields, duplicate/path rejection, no-follow regular-file checks, directory/link checks. | Same-user attackers can tamper with files and hashes together; inventories are corruption checks, not authenticated backups. |
| Confused-deputy/key leakage | Metadata-only request/response shapes reject paths, launch arguments, browsing/key operations and oversized/unknown requests. | No running Mojo service, per-profile key context, encrypted store or sandboxed coordinator yet. |

`./tools/stainless test` exercises these cases using fictional data, app manifests/processes
and native filesystem exchanges. The bundled-runtime integration tests execute the real Rust
helper from a synthetic app with no developer tools on its PATH, including create/list,
Standard/Incognito routing, concurrent ownership, pending recovery, failed spawn/exit/cleanup
and disconnected native output. Recovery tests add explicit consent/selection, bounded metadata
pages, newer/corrupt/incomplete snapshots, cold ownership/pending recovery and committed-but-
unacknowledged backup/restore results. Native Swift checks exercise helper selection, the real
subprocess transport, controls/actions, logical keyboard order and offscreen synthetic rendering.
A separate isolated-HOME journey runs the actual AppKit controls against the production Rust
helper: observed live startup, cold-operation admission after normal synthetic-browser exit,
verified backup, clone restore, original preservation, actionable corruption failure and
Safe Mode/explicit normal-return controls against the production runtime;
they never operate the user's desktop or browser.

These are not actual modified-browser, physical keyboard/screen-reader, clean-Mac or installed
package/update evidence. Every new browser build/package still requires fresh official upstream
provenance; the current source pin's separate evidence is in the
[build guide](build-macos.md#maintaining-chromium). Source/native fixtures
do not satisfy protected-data isolation or close #4/#6/#7/#8 and the Phase 1 release gates.

The browser smoke also contains a separate owned Standard -> Safe Mode -> normal journey:
a synthetic local extension/content script/background worker, retained startup/pinned-tab
preferences and session-file hashes, cookie/session-storage identity and normal restoration.
It requires the actual current **Safe Mode-capable** browser and matching ChromeDriver, not an older
candidate or flags-only mock. Native tests cover unsupported capability pairs, constrained
requests, ownership/pending updates, unsuccessful exit and missing Safe Mode acknowledgements.
The browser fixture explicitly replays a local `--load-extension` and Chromium's
`--restore-last-session` request in normal and safe launches; Safe Mode must suppress them.
It is not a native extension-installation or saved-extension autoload UI test. Chromium's
command-line/CDP extension loads are intentionally temporary, so those are not used as
evidence of a persistent installed-extension workflow.
