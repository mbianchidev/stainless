# stainless

An unblocked browser.

Stainless is a free, Chromium-based, developer-first browser combining auditable agent automation,
enhanced DevTools, optional local AI, clean browsing, and explicit identity isolation. It is funded
by donations and sponsorships, with no ads, sponsored placements, behavioural tracking, or mandatory
account.

No implementation exists yet — the repository currently holds the planning material for v1.

## Status

Planning. Work is tracked as issues parented by the
[epic](https://github.com/mbianchidev/stainless/issues/1), grouped into `Wave N` milestones and
labelled by `area:*`. No public or private binaries are distributed before complete v1 readiness.

## macOS launch troubleshooting

Stainless has no macOS implementation or distributed binaries yet. These steps are only for a future
macOS `.app` build whose source and integrity you have verified and trust, if Gatekeeper blocks launch
because of quarantine. They do not repair a corrupt build.

`Stainless.app` below is an expected name based on the project name, not a verified release artifact.
Use the actual installed bundle name if it differs.

1. Copy the `.app` bundle from its disk image or extracted archive into `/Applications`.
2. Open Terminal and remove quarantine from that bundle only:

   ```sh
   xattr -dr com.apple.quarantine "/Applications/Stainless.app"
   ```

3. Reopen Stainless from Applications.

This bypasses the quarantine launch check for that bundle; it does not establish that the app is safe.
Do not apply it to untrusted downloads or disable Gatekeeper globally.

## Documentation

- [`docs/roadmap.md`](docs/roadmap.md) — programme structure, delivery waves, global decisions,
  ownership boundaries, and resolved ambiguities.
- [`docs/decisions-q-and-a.md`](docs/decisions-q-and-a.md) — historical Q&A decision log. The
  roadmap supersedes it where they disagree.

## Contributing

The repository is private and has no license, contribution process, or vulnerability-disclosure
policy yet. All three are explicit decisions required before public release.
