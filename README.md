# Stainless

An unblocked browser.

Stainless is a planned, free, Chromium-based browser for developers, agent builders, and
privacy-conscious users. Its v1 design combines auditable agent automation, enhanced DevTools,
optional local AI, clean browsing, and explicit identity isolation. The intended funding model is
donations and sponsorships, without ads, sponsored placements, behavioural tracking, or a mandatory
account.

## Status

**Planning only. No browser implementation or downloadable release is available.**

The repository currently contains this README and two planning documents under `docs/`. It has no
application source, dependency manifests, bootstrap/build scripts, or tests. The capabilities below
are requirements for v1, not working features.

Work is tracked under the [v1 epic](https://github.com/mbianchidev/stainless/issues/1), grouped into
`Phase N` milestones and labelled by `area:*`. The
[execution plan](docs/roadmap.md#delivery-phases) starts with a usable vanilla Chromium browser for
Apple Silicon macOS, built locally and downloadable as an unsigned, unnotarized DMG, with automated
unsigned updates. Each later phase ships another usable browser with additional features;
completing all v1 features is not a prerequisite for previews. Signing and notarization come later.

**This is still a plan.** No phase has shipped. The current change updates Markdown and GitHub
planning only, with no application code, dependency installation, CI, or binary publication.

## Planned v1 capabilities

- **Auditable agents:** disposable, policy-controlled sandboxes with scoped capabilities, resource
  ceilings, and inspectable run evidence.
- **Enhanced DevTools:** replay network, DOM, and console activity; test and mock APIs; debug and
  profile WebAssembly.
- **Identity isolation:** separate profiles and immutable tab modes, including distinct Tor Proxy
  and Hardened Onion modes.
- **Optional AI:** local models and self-hosted endpoints, previewed context, and explicit consent
  for cloud use. Browsing must remain usable with AI disabled.
- **Clean browsing:** ad/tracker blocking and locally sanitized search results, with access to the
  original view.

Local-first operation, no automatic telemetry, and no required account or cloud AI are
[design requirements](docs/roadmap.md#global-decisions), not implemented guarantees.

## Platforms and prerequisites

The first build and downloadable-preview target is **macOS on Apple Silicon**. Linux and Windows
are planned desktop targets; mobile implementation is outside v1 scope.

Minimum OS versions, hardware requirements, and pinned toolchain versions are not specified yet.
The [build/bootstrap issue](https://github.com/mbianchidev/stainless/issues/5) tracks source setup.
The [macOS preview issue](https://github.com/mbianchidev/stainless/issues/37) tracks packaging,
download and installation; the [updater issue](https://github.com/mbianchidev/stainless/issues/6)
owns automatic update checks, downloads, and installation on a user-controlled restart from Phase 1.
These must work without a developer toolchain. There is currently no supported application
installation, bootstrap, build, or first-run command.

### Read the planning documents

To browse the documents on GitHub, you need access to this private repository. For a local copy,
you also need Git and SSH access to GitHub:

```sh
git clone git@github.com:mbianchidev/stainless.git
cd stainless
```

Start with [`docs/roadmap.md`](docs/roadmap.md). The checkout contains documentation only; cloning
does not install or launch a browser. No package installation is needed to read it.

## Usage, configuration, and scope

There is no runnable browser, CLI, settings UI, environment-variable contract, or configuration schema
yet. The roadmap's [global decisions](docs/roadmap.md#global-decisions) and
[resolved ambiguities](docs/roadmap.md#resolved-conflicts-and-ambiguities) describe intended
behaviour, not settings you can apply today.

Stainless is not planned as a full IDE, hosting platform, office suite, or live multi-user
collaboration tool. Local-service lifecycle belongs to Porto; Stainless plans only an optional
read-only adapter. See the [full non-goals](docs/roadmap.md#global-non-goals).

## macOS launch troubleshooting

**Future builds only.** Stainless has no macOS implementation or distributed binaries yet. These
steps are only for a future macOS `.app` build whose source and integrity you have verified and trust.
Preview packages are planned to have no Developer ID signature or Apple notarization. Checksums
detect corruption but do not independently authenticate the publisher.

`Stainless.app` below is an expected name based on the project name, not a verified release artifact.
Use the actual installed bundle name if it differs.

1. Copy the `.app` bundle from its disk image or extracted archive into `/Applications`.
2. Try opening the app. If macOS only reports an unidentified developer or missing notarization,
   follow Apple's [per-app exception guidance](https://support.apple.com/en-us/102445):
   **System Settings > Privacy & Security > Open Anyway**, then confirm.
3. If a verified, trusted unsigned Chromium build is blocked specifically by download quarantine,
   the targeted fallback is to remove quarantine from that bundle only:

   ```sh
   xattr -dr com.apple.quarantine "/Applications/Stainless.app"
   ```

4. Reopen Stainless from Applications.

The fallback bypasses that bundle's quarantine check; it does not establish that the app is safe or
repair damaged files. Do not override malware warnings or managed-device restrictions, apply this
to untrusted downloads, or disable Gatekeeper globally.

## Documentation

- [`docs/roadmap.md`](docs/roadmap.md): step-by-step delivery phases, per-phase release gates,
  global decisions, ownership boundaries, and resolved ambiguities.
- [`docs/decisions-q-and-a.md`](docs/decisions-q-and-a.md): historical Q&A that seeded the plan.
  It includes superseded answers and an unanswered question; the roadmap and issue bodies take
  precedence.
- [Roadmap and v1 epic](https://github.com/mbianchidev/stainless/issues/1): execution checklist and
  entry point to the implementation issues and their dependencies.

Individual issue bodies own capability scope, acceptance criteria, and non-goals. Milestones own
primary delivery-phase membership; the epic owns phase execution checklists, and the roadmap owns
the narrative and global decisions. Cross-cutting issues can span phases without blocking an
earlier, explicitly scoped release slice. There is no generated API reference or runnable example yet.

## Contributing

For planning feedback, start with the epic and the relevant capability's issue, then follow the
roadmap's [working conventions](docs/roadmap.md#working-conventions).

The repository is private and has no contribution guide or vulnerability-disclosure policy yet.
These are decisions required before public release, tracked in
[release documentation and open-source readiness](https://github.com/mbianchidev/stainless/issues/27).

## License

No license has been selected and no `LICENSE` file is present. A license must be chosen before public
release; do not assume the repository is open-source licensed.
