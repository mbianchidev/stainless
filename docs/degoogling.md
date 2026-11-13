# Google-service removal and feature availability

The published five-patch baseline targets **0.1.0-preview.4**. Earlier previews do not have this
configuration; check the installed version in **Stainless Updates** before assuming these
services are unavailable. Build/package evidence and private release availability remain
separate from the source implementation.
Local **preview.5** builds add a sixth patch removing the unavailable service settings and adding
managed-mode enforcement. Actual isolated-browser settings/traffic checks pass for this explicitly
requested Chromium 158 experiment. It is not published; existing previews do not gain these UI
changes automatically. The [current Stable source evidence](build-macos.md#maintaining-chromium)
is separate from that preserved candidate and does not authorize publication.

**Google Safe Browsing, phishing/download reputation and password-leak checks are removed
without a replacement threat service.** This preview has fewer automated threat warnings.
Normal TLS validation, certificate error pages, site isolation and the native renderer
sandbox remain required. Removing Google services is not a complete privacy or security product.

## Removed services and replacement gaps

| Removed integration | Current consequence | Future owner |
| --- | --- | --- |
| Safe Browsing, remote phishing/download reputation and breach checks | No Google-backed threat or compromised-password warnings. No replacement lists/lookups are implemented. | [#9](https://github.com/mbianchidev/stainless/issues/9), [#11](https://github.com/mbianchidev/stainless/issues/11), [#26](https://github.com/mbianchidev/stainless/issues/26) |
| Browser Google sign-in/sync and cloud profile/reporting management | Local profiles remain usable; Google account sync is unavailable. | [#7](https://github.com/mbianchidev/stainless/issues/7), [#12](https://github.com/mbianchidev/stainless/issues/12), [#10](https://github.com/mbianchidev/stainless/issues/10) |
| Translate, remote Web Speech, Live Caption/Live Translate, Screen AI and image-label services | No Google translation, transcription, captions or downloaded accessibility models. OS accessibility, keyboard dictation and ordinary microphone/WebRTC access are separate. | [#40](https://github.com/mbianchidev/stainless/issues/40), [#23](https://github.com/mbianchidev/stainless/issues/23) |
| Built-in Google search prepopulation, provider suggestions, navigation prediction and remote error corrections | DuckDuckGo is the fallback. Remote suggestions/prediction are disabled for all providers; local history/bookmark suggestions and explicit custom providers remain separate. | [#8](https://github.com/mbianchidev/stainless/issues/8), [#13](https://github.com/mbianchidev/stainless/issues/13) |
| Enhanced spellchecking, Autofill crowdsourcing and payment autofill | Native/local spelling, local passwords and address heuristics remain independent. Remote form predictions and card autofill are unavailable. | [#11](https://github.com/mbianchidev/stainless/issues/11), [#13](https://github.com/mbianchidev/stainless/issues/13) |
| Google component/model delivery, variations/Finch and network-time querying | No automatic Google component/model/experiment/time service. Bundled component/security data must be refreshed through reviewed browser releases; currentness cannot be inferred from browsing working. | [#6](https://github.com/mbianchidev/stainless/issues/6), [#23](https://github.com/mbianchidev/stainless/issues/23), [#25](https://github.com/mbianchidev/stainless/issues/25) |
| Google extension updater and FCM/GCM-backed Web Push | Extension update checks are unavailable for all providers. Current source removes the update control; API calls still fail explicitly. Local packages/per-item reload remain separate. Push subscriptions have no working transport. | [#15](https://github.com/mbianchidev/stainless/issues/15), [#41](https://github.com/mbianchidev/stainless/issues/41) |
| Google Cast, Google-backed generative features, network geolocation, Privacy Sandbox advertising, metrics/reporting and feedback/promotions | Disabled/unavailable, with no private services silently substituted. | [#9](https://github.com/mbianchidev/stainless/issues/9), [#10](https://github.com/mbianchidev/stainless/issues/10), [#23](https://github.com/mbianchidev/stainless/issues/23), [#25](https://github.com/mbianchidev/stainless/issues/25) |
| Google secure-DNS presets/upgrades and captive-portal probes | Google DNS/DNS64 presets are removed. Other/custom DNS providers remain separate; the OS/network resolver is still the user's configuration. Automatic Google portal detection is unavailable and does not report a successful connectivity test. Manual portal navigation remains possible. | [#9](https://github.com/mbianchidev/stainless/issues/9), [#25](https://github.com/mbianchidev/stainless/issues/25) |
| Google macOS updater and primary help routes | About Stainless explains the separate **Stainless Updates** menu instead of installing/checking Google's updater. Main help links open the Stainless macOS guide. Private GitHub updates still require opt-in authorization. | [#6](https://github.com/mbianchidev/stainless/issues/6), [#27](https://github.com/mbianchidev/stainless/issues/27) |
| Google distribution/brandcode reset-settings lookup | Profile reset uses local factory defaults instead of downloading distribution preferences from Google. No remote reset configuration or distribution-specific extension list is fetched. | [#8](https://github.com/mbianchidev/stainless/issues/8), [#25](https://github.com/mbianchidev/stainless/issues/25) |

[The removal inventory](https://github.com/mbianchidev/stainless/issues/42) tracks replacement
ownership and verification. Existing capability issues remain the scope authority. A gap does
not automatically expand v1, choose a delivery phase, or make an unimplemented feature available.
Replacements need an explicit design/delivery decision, verified assets/provider trust,
synthetic tests and clear unavailable/error behavior.

## Enforcement and compatibility

The build excludes PDF-to-Google-Drive integration. Browser sign-in is disabled by the
immutable product policy and empty Google OAuth configuration; `enable_dice_support` is a
derived upstream value, not an overrideable GN argument in this pin.
The full Chromium graph requires Safe Browsing code, so its state/standard/enhanced
availability gates and built-in protection policy are disabled instead of generating
unresolved browser/test dependencies. The pinned desktop GN graph requires supervision support
to compile; downstream gates disable Google Family Link activation and its remote safe-sites
service instead. OS/device parental controls are separate and are not replaced or disabled.
The Chromium 158 tree reads safe-sites settings through `FamilyLinkSettingsService`; its shared
accessor remains disabled even for cached settings, independently of device parental controls.
The full browser PDF integration likewise requires Screen AI code to compile; its OCR/main-content
availability gates are disabled, so no Screen AI service/model download is made available.
Speech support is also required by the desktop build graph; cloud recognition is denied and
Web Speech's service receiver is closed with an explicit unavailable reason. Caption/translation
policies and Google component delivery remain disabled.
Downstream service gates
disable remaining Google request paths. Immutable built-in restrictions are visible in
`chrome://policy` with **Stainless** as their source, not a fictitious enterprise enrollment.
Browser account-cookie listening, listing and logout are unavailable without changing
cookies for user-directed websites. Component delivery has no background scheduler or
on-demand network path; current source removes its update action while retaining an
unavailable status and a failing API. These restrictions
remain independent of the `disable-component-update` switch and update preferences.
Old user preferences, normal policy precedence and experimental flags must not silently
enable removed services. No cloud enrollment or telemetry endpoint is added.

This is not a global Google-domain firewall. User-directed visits to Google sites, content
requested by those sites, deliberate custom providers, and OS/network DNS configuration
remain user choices. There is no claim that all traffic on the Mac avoids Google.
Chromium/depot_tools downloads and security-pin checks are build-host operations, not browser
telemetry.

Stainless's product copyright and publisher identity are **Synctune Open**.
Genuine Chromium and third-party copyright/license/project acknowledgments stay intact in
the included notices and project credits. Standard `chrome://`
URLs, Chrome user-agent tokens, DevTools/ChromeDriver protocol identifiers and external product
names are retained where compatibility or accurate attribution requires them. Product UI,
menus, dialogs and app identity use **Stainless**. Rebranding happens after translation so
upstream translation IDs, placeholders and localized grammar are preserved, including
messages included from settings/shared resource files. The local-profile section is named
**Profiles**, and the unavailable Google generative-settings page is not offered.

The app, native helpers and WebUI product images use a black icon with the original silver S
and accent colors unchanged. The transparent master logo remains available separately.
OS template/status masks retain the S's alpha shape instead of becoming a solid square.

## Settings removal in the current source

The sixth patch removes controls and their active bindings, not disabled switches or
CSS-hidden service pages. Removed routes fall back to the settings root; search cannot
resurrect their views. Required internal preferences, resources and compiled dependencies
may remain, independently of whether a product setting exists.

| Surface | Removed settings/actions | Retained local or ordinary functionality |
| --- | --- | --- |
| Profiles | Browser sign-in/sync, account/service/encryption pages, account-management links and sign-out dialogs | Account-free profile name/picture editing and explicit import |
| Security/privacy | Safe Browsing choices/reporting and its automatic JavaScript-optimizer mode, breach-check toggle, Google Advanced Protection enrollment, service-oriented Privacy Guide and remote Safety Hub cards | Passive unavailable/cached-compromise warnings; HTTPS, certificates, custom secure DNS, security keys, manual/per-site JavaScript optimizer choices, cookies, permission and extension review |
| Languages/accessibility | Translate card/target indicators; cloud captions, image labels and downloaded-model annotation controls | Language order, native spelling, OS captions/accessibility, focus/caret/page-color controls and trackpad settings. Enhanced spellchecking was already excluded from this unbranded Mac UI |
| Autofill | Payment/Wallet/account cards, AI/Gemini and shopping opt-ins, Gmail OTP settings and cloud-wallet record actions | Local addresses/passwords and review/edit/delete of existing local saved document/travel records. This does not enable enhanced filling or a new local AI service |
| Performance/services | Preloading controls, remote suggestions/URL-collection/metrics settings exposed through the removed service pages | Local memory/battery/resource controls and the existing optional CPU settings |
| Password Manager | Breach-check navigation, Google vault/enclave/PIN, move-to-account, sharing and automated password-change controls | Local password saving, website auto-sign-in, biometric filling, OS passkey management, CSV import/export and explicit conflict resolution |
| Components/extensions | Component and extension update buttons and obsolete bulk-update UI handlers | Explicit backend denial; bundled-data status, local extension packages and per-item reload |

This includes `/syncSetup`, `/syncSetup/advanced`, `/account`, `/googleServices`,
`/signOut`, the Google-backed `/ai` subtree, `/payments`, `/shopping`,
`/enhancedAutofill`, `/privacy/guide`, `/content/location` and `/content/ads`.
Password Manager also rejects its removed checkup/automated-change routes.
Identity-document/travel routes remain for existing **local** records, without Google
opt-ins or Wallet actions. No records are deleted as part of this settings cleanup.
Generic website notifications, Background Sync and payment-handler permissions are not
Google browser sync, FCM transport or Google Wallet; those ordinary controls remain.

The immutable restrictions and service gates are not removed with their UI. Gmail OTP's
backend factory additionally selects no profiles and constructs no service, including when
old preferences or experimental flags remain. There is no new OAuth configuration or API key.
Guest and future mode controls stay unavailable; the [managed-mode matrix](profile-isolation.md#managed-browsing-modes)
does not claim OS-enforced same-user isolation.
The seventh patch adds [explicit Standard startup Safe Mode](profile-isolation.md#managed-startup-safe-mode),
not another Google-service/privacy mode. Its temporary extension/background/restoration
restrictions do not remove local passwords or repeatable import/export, enable browser sync,
add an account requirement or weaken sandboxing/site isolation/TLS. Old candidates do not
gain it automatically.

### Repeatable explicit import and export

Data transfer is user-triggered and **repeatable**, not a permanently single-use migration
and not continuous/background Google sync.

- **Bookmarks:** retain the browser's HTML export and explicit HTML-file import. Re-imports
  can merge/create imported folders or duplicates; they do not reconcile deletions or
  automatically replace stale entries. Review the imported data.
- **Passwords:** retain local CSV export/import, source-file handling and the existing
  conflict preview. Replace only selected conflicts or skip them. Every new import clears
  previous replacement selections and error summaries; account state cannot select a cloud
  destination. Exported CSVs are sensitive plaintext; source-file deletion requires consent.
- **Other browsing data:** the native source/category picker remains available for categories
  supported by that detected source. This slice adds no universal history/cookie/tab export,
  new source format or silent profile migration. The separate preview-copy/restore tool
  retains its existing ownership and compatibility checks.

Ordinary login to Google websites remains possible. **Website login does not expose Chrome
sync bookmarks, passwords or other browser-sync data to Stainless.** Use a supported export
from the source, then explicitly import its supported local file. No working signed-in Google
cloud importer is supplied or implied. Unsupported sources/formats must not be reported as
successful imports.

## Verification and limits

`./tools/stainless test` covers synthetic protocol/installer behavior and native icon generation.
The icon test checks every visible master-logo pixel for unchanged color and validates the
black background, transparent corners and 1024px Retina frame.

`./tools/stainless smoke /exact/path/to/Stainless.app` checks actual About/Version branding,
built-in policy source, browsing/persistence, downloads/PDF, Incognito separation and TLS.
It inspects `sandbox_check` only for ChromeDriver-owned headless renderer children and
checks a synthetic cross-origin out-of-process iframe. About-page screenshots are captured
from that isolated browser, never from the user's desktop.
It excludes ChromeDriver's own background-service suppression, records first-run diagnostics,
and observes 30 seconds of idle time plus the synthetic browsing sequence. Missing or invalid
NetLog evidence fails the check; recorded Google-service URL requests fail it.
This is a bounded observation, not all-time monitoring or proof about every DNS/socket event.

The current smoke also checks settings routing/search, retained local controls, explicit
backend rejection after update-button removal, and a fresh-root forced-Incognito session
against contradictory user preferences. Its repeat-import UI check mocks the import-service
boundary so it never enumerates the host's installed browser profiles or opens a native file
picker. It is not evidence of a native file import or a functioning Google cloud importer.
These browser checks also pass on the separately built **158.0.8127.37 Mac Stable source**
with all six patches. No new candidate was packaged. This does not revalidate the preserved
preview.5 candidate or older published DMGs, or verify installed native workflows.
The current source pin's fresh Stable provenance, actual browser evidence and binary hashes
are recorded separately in the [build guide](build-macos.md#maintaining-chromium).

The local preview.5 native managed-profile chooser is account-free and uses bundled local helpers.
It adds no Google-service/sign-in/sync controls or cloud importer. Its synthetic AppKit/Rust
checks do not replace the modified-browser and installed-package evidence above.

Source-only routing and CSV-import fixtures execute the production TypeScript with synthetic
browser/service boundaries. They cover removed deep links, retained local routes, local-only
destinations, repeat imports, fresh conflict consent and source-file retention. They are not
rendered-browser evidence. No low-signal tests were removed.

Record source/patch hashes, installed candidate hashes, the observation window and residual
limitations with release evidence. A separate clean Mac/user, remaining native workflows and
actual authorized automatic N-to-N+1 download/restart remain required before calling
[Phase 1 complete](phase-one.md#release-evidence).
