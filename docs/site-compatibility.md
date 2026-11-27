# Site compatibility and the Stainless user agent

*Current source.* Installed previews do not have this yet.

Stainless tells websites it is Stainless. If a site only works in Google Chrome, you can
present as Google Chrome to that one site for a limited time. Every override expires on its
own. This is a compatibility tool, not a privacy or anonymity feature.

## The default identity

Stainless keeps Chromium's reduced user agent and adds one product token with only the
Chromium major version, for example:

```text
Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/159.0.0.0 Safari/537.36 Stainless/159.0.0.0
```

User-agent client hints (`Sec-CH-UA` and `navigator.userAgentData.brands`) name `Stainless`
next to `Chromium` and the usual randomized placeholder brand. The token reveals that the
browser is Stainless but not which preview or build. A `--user-agent` command-line value
still replaces the string completely, as in Chromium.

## Present as Google Chrome to one site

1. Open the site, then choose **View > Site Compatibility...** or the same item in the
   browser menu.
2. Choose **1 day**, **7 days** or **30 days**, then **Present as Google Chrome**. The tab
   reloads with the new identity.
3. **Use Stainless Identity** removes the override for the open site. **Remove Selected**
   removes any override in the list.

An override covers the site's registrable domain, so `shop.example.com` and
`www.example.com` share one override for `example.com`. IP addresses and hosts without a
registry, such as `localhost`, use the host itself. Only `http` and `https` sites can have one.

While an override applies, requests, `navigator.userAgent` and client hints for that tab
match Google Chrome's reduced user agent: the `Stainless/...` token is left out and the
client hints brand is `Google Chrome` instead of `Stainless`. A tab notice names the site
and the expiry date the first time it loads in that tab.

## What is recorded

Overrides live in the profile's preferences under `stainless.site_compatibility.overrides`.
They are local and never synced. Each entry records:

- the site, the creation time and the expiry time (at most 30 days later);
- the change it makes, shown in the dialog: the token removed and the brand claimed.

Expired or malformed entries are ignored and removed the next time the dialog opens.

## Limits

- **Not anonymity.** Cookies, storage, extensions, fonts, screen size and every other trait a
  site can measure stay the same. Presenting as Chrome blends one header and one API with a
  larger population; it does not hide who you are. Fingerprint protection is tracked in
  [#30](https://github.com/stainless-browser/stainless/issues/30).
- **The distinct token is a small fingerprinting cost.** Sites can tell Stainless users
  apart from Chrome users by default. The token carries no version beyond Chromium's major
  version.
- **When it applies.** The override is chosen when a navigation starts in the tab. A
  navigation that starts on another site and redirects to the overridden one keeps the
  Stainless identity until the next navigation there. Subframes in that tab follow the
  top-level page.
- **Workers.** Service workers and shared workers keep the default identity.
- **Private windows.** Incognito, managed Incognito and Guest always send the Stainless
  identity; the dialog is unavailable there.
- **Misrepresentation.** A site may behave differently, or log you as a Chrome user.

## Evidence

`StainlessSiteCompatibilityBrowserTest` in `stainless_command_palette_tests` checks the
default identity, that an override changes the header, `navigator.userAgent` and client hints
for its site only, expiry and input limits, the dialog's add and remove actions with a reload,
and that private windows keep the Stainless identity. The
[built-app smoke](build-macos.md#local-commands) requires the default token and brand. Run
the browser tests only in an
[isolated macOS GUI environment](build-macos.md#native-macos-gui-test-isolation).
