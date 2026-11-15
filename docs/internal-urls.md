# Stainless internal addresses

Current source accepts **`stainless://settings/people`** in the address bar.
Settings navigation, reload, back/forward and native Settings/library commands
keep the Stainless address visible. The local Profiles roster and explicit
import remain the existing native Settings features, not a second settings app.
Older installed previews do not acquire this behavior automatically.

## Supported pages

The browser-side alias supports these hosts, including their native paths,
queries and fragments:

`settings`, `password-manager`, `history`, `downloads`, `bookmarks`,
`extensions`, `version`, `policy`, `components`, `credits`,
`certificate-manager`, `chrome-urls`, `about` and `help`.

Native redirects still apply. For example, `stainless://help/` displays
`stainless://settings/help`, and `/settings/addresses` becomes
`/settings/contactInfo`. Removed Google-service settings still return to the
local Settings root. No sync, account, cloud-import or removed extension
capability is restored. Incognito and managed forced-private routing retain
the same restrictions as their canonical addresses.

Legacy **`chrome://` addresses still work**. Supported native pages display the
Stainless spelling after navigation commits. Browser-owned links can retain
canonical `chrome://` destinations internally; their resulting address uses
the same alias. A Stainless alias resolving to an extension override or a
destination outside the supported native pages displays its actual destination,
not a Stainless label over unrelated content. Existing `chrome://` extension
overrides retain their native compatibility behavior.

## Origins and restrictions

This is an address alias, **not a renamed privileged origin**. The Settings
document, JavaScript `location`, resource imports, WebUI controller/bindings,
CSP and renderer/site-isolation checks still use `chrome://settings`.
Native page identity/page-info and exclusion from ordinary browsing history
also retain their canonical behavior.
DevTools, ChromeDriver, protocol identifiers, user-agent tokens and Chromium
credits remain compatible and accurately attributed.

`stainless` is reserved for in-browser navigation. It is not registered with
macOS, a website or an external application, or added to renderer-safe,
secure, WebUI, CORS or resource-loader schemes. Websites and extension
renderers gain no ability to request or serve privileged content through it.
Existing canonical WebUI links and same-document Settings routes remain the
renderer navigation path.

Other hosts, resource-only endpoints, debug/action URLs, credentials and ports
are not aliased. In particular, hosts `crash`, `quit`, `restart`,
`resources`, `theme`, `gpu` and `newtab` do not invoke those native handlers.
Unsupported addresses fail locally instead of opening an external handler,
resolving a network host or falling back to search. Syntactically malformed
addresses typed in the address bar use the local error destination
`stainless://invalid/`.
No `stainless-untrusted` scheme or `view-source:stainless` support is added;
developer/resource/debug compatibility remains under its existing canonical
schemes.

## Verification boundaries

The focused Chromium fixture in `stainless_command_palette_tests` exercises
the actual visible address/model, canonical origin, typed navigation, native
commands, deep links, reload/history, unavailable routes, local errors,
renderer request/CSP restrictions and forced Incognito. The existing
local-profile roster fixture uses the new address. The full-browser smoke
also opens Settings through it and retains the removed-service/import checks.

The updater's separate offscreen AppKit fixture checks its unchanged S icon,
empty adjacent title, accessible identity and tooltip across ready, busy,
error and recovery states, plus visible failure for missing artwork.
These checks do not establish physical VoiceOver/keyboard behavior, an
installed feature, clean-Mac installation or an authorized automatic update.
The [release gates](phase-one.md#release-evidence) remain separate.
