# Threat lists and password breach checks

*Current source.* Installed previews up to preview.8 do not have these features.

Stainless removes Google Safe Browsing and Google's password-leak service
([feature availability](degoogling.md)). Current source replaces part of that protection
with two features that never send the sites you visit to anyone:

| Feature | Default | What leaves this Mac |
| --- | --- | --- |
| Block known phishing and malware sites | On | Plain HTTPS downloads of two public domain lists. No visited URL, cookie or credential. |
| Check passwords against known data breaches | Off (opt-in) | The first 5 characters of a password's SHA-1 hash, sent to Have I Been Pwned. |

Neither replaces download scanning, reputation checks for new pages or a full Safe Browsing
service. Normal HTTPS certificate validation, site isolation and the renderer sandbox stay on.

## Threat lists

**Settings > Privacy and security > Security > Block known phishing and malware sites**
turns this on or off for every profile of the Stainless install. The same page shows the
loaded domain count, each list's license, size and last update, the last download error and
an **Update lists** button.

### Sources

| List | License | Download |
| --- | --- | --- |
| [HaGeZi Threat Intelligence Feeds](https://github.com/hagezi/dns-blocklists) (mini, domains only) | GPL-3.0 | jsDelivr, then raw.githubusercontent.com, Codeberg and GitLab mirrors |
| [CERT Polska warning list](https://cert.pl/en/warning-list/) | Free to use, attribution not required | `hole.cert.pl` |

Stainless downloads the lists as data at runtime and does not ship or modify them.

### Updates and storage

- The first download starts about 30 seconds after the browser starts. Lists then refresh
  every 6 hours while protection is on, and failed downloads retry after 30 minutes.
- Requests use HTTPS only, send no cookies or credentials, skip the HTTP cache and use
  `If-None-Match` so unchanged lists are not downloaded again. Redirects to plain HTTP are
  rejected. Each list host sees the browser's IP address and User-Agent.
- Files live in `Stainless Threat Lists/` inside the user-data folder
  (`~/Library/Application Support/Stainless`), with `metadata.json` holding ETags and
  last-success times.
- A download replaces the cached list only after it parses with a minimum number of valid
  domains (10,000 for HaGeZi, 1,000 for CERT Polska) and stays under 32 MiB. A failed,
  truncated or malformed download keeps the previous list active.
- Turning protection off stops downloads and matching. Cached files stay on disk; delete the
  folder above to remove them.

### Matching

Every page and frame navigation, including redirects, is checked on this device. Images,
scripts and other subresources are not. A host matches when it or a parent domain, down to its registrable domain, is on a
list. Public suffixes (such as `co.uk` or `github.io`) and IP addresses never match. Domains
are kept in memory as 64-bit SHA-256 prefixes.

A matching page shows a **Stainless blocked a dangerous site** warning with the listed
domain and a **Back to safety** button. **Visit this site anyway** allows that listed domain
for the rest of the browser session in that profile; nothing is saved. Listed subframes,
prerenders and fenced frames are cancelled without a warning. Lists can contain mistakes;
the toggle above turns the protection off.

## Password breach checks

**Password Manager > Settings > Check passwords against known data breaches** is off by
default. When on, Stainless uses the
[Have I Been Pwned Pwned Passwords range API](https://haveibeenpwned.com/API/v3#PwnedPasswords):

1. Stainless computes the SHA-1 hash of the password on this Mac.
2. It sends only the first 5 hexadecimal characters to
   `https://api.pwnedpasswords.com/range/` with `Add-Padding: true`, so responses have a
   uniform size. No API key, cookie, username or site is sent.
3. It compares the remaining 35 characters with the returned list locally. Padding rows
   with a count of 0 are ignored.

Checks run only after an explicit action:

- **After you sign in to a site** with a password Stainless sees submitted. A breached
  password shows the standard "Change your password" dialog.
- **When you edit a saved password** in Password Manager, for that password only.
- **When you open Checkup or press its refresh button** in Password Manager. Up to 4 range
  requests run at a time. Breached saved passwords appear under **Compromised passwords**.

Safety Hub's background schedule only runs the local weak and reused password checks; it
never contacts Have I Been Pwned. With breach checks off, Checkup still finds weak and
reused passwords and says breach checks are off instead of reporting "no compromised
passwords". A network or service error is shown as a failed check, never as a clean result.

## Verification

`stainless_privacy_unittests` (built with `./tools/stainless build --native-tests`) covers
list parsing and normalization, minimum-entry rejection, parent-domain and public-suffix
matching, cross-list de-duplication, SHA-1 prefix/suffix splitting for ASCII and non-ASCII
passwords, padded responses and malformed responses. It starts no browser or AppKit UI.
Browser-level checks use only synthetic lists, domains and passwords.
