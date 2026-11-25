# Security policy

## Supported versions

Only the newest published [preview release](https://github.com/stainless-browser/stainless/releases)
receives security fixes. Older previews, local builds and unpublished source are unsupported;
update to the newest release before reporting.

## Report a vulnerability

Report privately through
[GitHub private vulnerability reporting](https://github.com/stainless-browser/stainless/security/advisories/new)
(**Security > Report a vulnerability**). Do not open public issues, pull requests or
discussions for suspected vulnerabilities.

Include the affected version/build from **About Stainless**, the macOS version, reproduction
steps and the expected impact. Use synthetic data only: never include real credentials,
tokens, browsing history or personal data.

In scope: Stainless patches, the native updater and menu-bar helper, release tooling and
published packages. Documented preview limitations are not vulnerabilities by themselves,
including unsigned/unnotarized packages, removed Google Safe Browsing, the
[limits of threat-list protection](docs/threat-protection.md) and other
[replacement gaps](docs/degoogling.md), and the [updater trust limits](docs/install-macos.md).

## Upstream Chromium issues

If the issue reproduces in unmodified Chromium, report it through
[Chromium's security process](https://www.chromium.org/Home/chromium-security/reporting-security-bugs/).
Stainless receives those fixes by tracking the latest official Mac Stable release
([maintenance workflow](docs/build-macos.md#maintaining-chromium)).

## What to expect

Stainless has a single maintainer and handles reports on a best-effort basis. There is no
staffed response guarantee or bug bounty. Fixes ship in a new preview release, followed by a
GitHub security advisory crediting the reporter unless they prefer otherwise. Please allow up
to 90 days before public disclosure, or agree a different timeline in the private report.
