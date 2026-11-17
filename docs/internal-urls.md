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

Availability still follows the canonical native page; aliases do not enable
disabled or unavailable features.

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
Native code must distinguish `WebContents::GetLastCommittedURL()` (the virtual
address) from the frame/entry's canonical URL when checking origin or page type.
Trust checks must also use the committed origin and error-document state.
DevTools, ChromeDriver, protocol identifiers, user-agent tokens and Chromium
credits remain compatible and accurately attributed.

`stainless` is reserved for in-browser navigation. Isolated apps and saved
custom protocol handlers cannot claim it or make it renderer-safe.
It is not registered with macOS, a website or an external application, or added
to renderer-safe, secure, WebUI or CORS schemes. A rejecting, navigation-only loader turns
unrewritten addresses into local error pages; it serves no resources.
Websites and extension renderers gain no ability to request or serve privileged
content through it.
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

**Verified source, 2026-11-15:** clean commit
`1ee39fc28381b04c09ba77838b6effe05390852e` builds the complete fourteen-patch
browser, matching ChromeDriver and native helpers with eight browser build jobs
on Apple Silicon macOS 26.6, Xcode/SDK 27.2 and Rust 1.99.1. The separate cache
is `~/Library/Caches/StainlessBuildIdentity-7935dc80/`; the app's adjacent
`stainless-build.json` records that commit with `dirty: false` and all fourteen
patch hashes. The thirteen earlier patch files, tool/GN/product/schema pins,
previous source caches and preview candidates are unchanged.

Fresh [official Mac Stable metadata](https://chromiumdash.appspot.com/fetch_releases?channel=Stable&platform=Mac&num=1)
and [source VERSION](https://chromium.googlesource.com/chromium/src/+/b8c2be851f62f6a394ec57955d7f31335d7cb6d4/chrome/VERSION?format=TEXT)
still agree on **158.0.8127.37** /
`b8c2be851f62f6a394ec57955d7f31335d7cb6d4` at **2026-11-15T19:39:43Z**.
The new patch's SHA-256 is
`d2c73094e61b1861db0993f6d36462f39ac1166fc7a31cec5de1a4756eeae920`.

All **54 native result cases plus five PRE stages pass in three complete
launcher runs**, with retries disabled: **177 successful launched tests**.
This retains the palette, shortcuts/keymaps, resource overview, saved-history
privacy and local-profile journeys alongside the new alias cases.
The 98 locked Rust tests, existing offscreen AppKit/recovery checks, formatting,
Clippy, release-tool build, source settings/import fixtures, RustSec audit and
strict ordered patch checks also pass. Five opt-in real-payload tests still
need authorized release packages and remain unrun. No meaningful tests were
removed; the previously documented upstream WebUI aggregate gap is not waived
by the dedicated fixture.

| Verified binary | SHA-256 |
| --- | --- |
| Browser launcher | `5b70f251e8084e76a239e1123a5391ab00a4e994e77c5d62e0857176bfda74cd` |
| Active Stainless Framework | `5f6316e72f60ea7c580f3ad3de01c50d6e77c21d0f8c4b22b54183d5f9442137` |
| Matching ChromeDriver | `f6f2de1ae8255d943b78621176a5ab32e04803fafbf9f7d3f3c05415eca6aefc` |
| Native browser fixture | `4f637d30b496ff8ce2741496bfd2ac9825f033d970b9f249381e6175c2f37344` |
| Native updater executable | `378de3f69dac302863ba886077ebaf3333e105f14f4312f1e957f62d6aa16600` |

The focused Chromium fixture in `stainless_command_palette_tests` exercises
the actual visible address/model, canonical origin, typed navigation, native
commands, deep links, reload/history, unavailable routes, local errors,
renderer request/CSP restrictions and forced Incognito. The existing
local-profile roster fixture uses the new address. The full-browser smoke
also opens Settings through it and retains the removed-service/import checks.
The final exact app/driver smoke passes ordinary browsing, downloads,
DevTools/PDF, persistence, private/Safe Mode restrictions, rejected self-signed
TLS and trusted HTTPS, an out-of-process cross-origin iframe and six owned
renderers checked with `sandbox_check`. It records zero Google-service URL
requests during the bounded 30-second idle window plus browsing sequence,
not an all-time no-network guarantee.

The updater's separate offscreen AppKit fixture checks its unchanged S icon,
empty adjacent title, accessible identity and tooltip across ready, busy,
error and recovery states, plus visible failure for missing artwork.
These checks do not establish physical VoiceOver/keyboard behavior, an
installed feature, clean-Mac installation or an authorized automatic update.
Headless runs retain display/CVDisplayLink fallback and occasional child-reaping
diagnostics. The unchanged release crate reports an unused `serde_json`
dependency warning in a full workspace release build; native Apple stripping
and signature verification pass without the former rust-objcopy warnings.
No installer, package, installation or publication is produced. Licensing and
the [release gates](phase-one.md#release-evidence) remain open.
