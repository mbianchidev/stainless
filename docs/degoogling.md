# Google-service removal and feature availability

The five-patch baseline targets **0.1.0-preview.4**. Earlier previews do not have this
configuration; check the installed version in **Stainless Updates** before assuming these
services are unavailable. Build/package evidence and private release availability remain
separate from the source implementation.

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
| Google extension updater and FCM/GCM-backed Web Push | Extension update checks are unavailable for all providers. The update control is disabled and API calls fail explicitly. Local extension packages/per-item reload are separate from updates. Push subscriptions have no working transport. | [#15](https://github.com/mbianchidev/stainless/issues/15), [#41](https://github.com/mbianchidev/stainless/issues/41) |
| Google Cast, Google-backed generative features, network geolocation, Privacy Sandbox advertising, metrics/reporting and feedback/promotions | Disabled/unavailable, with no private services silently substituted. | [#9](https://github.com/mbianchidev/stainless/issues/9), [#10](https://github.com/mbianchidev/stainless/issues/10), [#23](https://github.com/mbianchidev/stainless/issues/23), [#25](https://github.com/mbianchidev/stainless/issues/25) |
| Google secure-DNS presets/upgrades and captive-portal probes | Google DNS/DNS64 presets are removed. Other/custom DNS providers remain separate; the OS/network resolver is still the user's configuration. Automatic Google portal detection is unavailable and does not report a successful connectivity test. Manual portal navigation remains possible. | [#9](https://github.com/mbianchidev/stainless/issues/9), [#25](https://github.com/mbianchidev/stainless/issues/25) |
| Google macOS updater and primary help routes | About Stainless explains the separate **Stainless Updates** menu instead of installing/checking Google's updater. Main help links open the Stainless macOS guide. Private GitHub updates still require opt-in authorization. | [#6](https://github.com/mbianchidev/stainless/issues/6), [#27](https://github.com/mbianchidev/stainless/issues/27) |

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
The full browser PDF integration likewise requires Screen AI code to compile; its OCR/main-content
availability gates are disabled, so no Screen AI service/model download is made available.
Speech support is also required by the desktop build graph; cloud recognition is denied and
Web Speech's service receiver is closed with an explicit unavailable reason. Caption/translation
policies and Google component delivery remain disabled.
Downstream service gates
disable remaining Google request paths. Immutable built-in restrictions are visible in
`chrome://policy` with **Stainless** as their source, not a fictitious enterprise enrollment.
Old user preferences, normal policy precedence and experimental flags must not silently
enable removed services. No cloud enrollment or telemetry endpoint is added.

This is not a global Google-domain firewall. User-directed visits to Google sites, content
requested by those sites, deliberate custom providers, and OS/network DNS configuration
remain user choices. There is no claim that all traffic on the Mac avoids Google.
Chromium/depot_tools downloads and security-pin checks are build-host operations, not browser
telemetry.

Genuine Chromium copyright/license/project acknowledgments stay intact. Standard `chrome://`
URLs, Chrome user-agent tokens, DevTools/ChromeDriver protocol identifiers and external product
names are retained where compatibility or accurate attribution requires them. Product UI,
menus, dialogs and app identity use **Stainless**. Rebranding happens after translation so
upstream translation IDs, placeholders and localized grammar are preserved.

The app, native helpers and WebUI product images use a black icon with the original silver S
and accent colors unchanged. The transparent master logo remains available separately.
OS template/status masks retain the S's alpha shape instead of becoming a solid square.

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

Record source/patch hashes, installed candidate hashes, the observation window and residual
limitations with release evidence. A separate clean Mac/user, remaining native workflows and
actual authorized automatic N-to-N+1 download/restart remain required before calling
[Phase 1 complete](phase-one.md#release-evidence).
