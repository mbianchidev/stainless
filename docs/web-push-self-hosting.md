# Optional, user-controlled Web Push

This is a **local interoperability fixture**, not a public deployment, a browser
release, or evidence that Stainless service workers/native notifications work.
It creates no account, installs no desktop software, changes no system trust,
and does not select a Mozilla, Google or Stainless delivery service.

The server is **Autopush 1.86.0 plus the reviewed downstream patches in
`tools/push_fixture/patches/`**, not stock-source evidence. Both server binaries
are built with `--no-default-features --features postgres`. Upstream calls
Postgres support **`enterprise-beta`**; do not describe it as production-stable.
All source, dependency, build-tool and OCI inputs are pinned by digest in
[`inputs.json`](../tools/push_fixture/inputs.json).

## What runs

- Autoconnect: Autopush-compatible WSS user-agent transport.
- Autoendpoint: standard VAPID/encrypted Web Push publishing.
- Non-Google Postgres 18, with persistent, fixture-owned storage.
- Caddy 2.11.9: supplied-certificate TLS and WebSocket reverse proxy. Automatic
  HTTPS/ACME, its admin API and access logging are disabled.

The publishing origin and connection URL use **different loopback ports**.
Never configure the publishing origin as a WebSocket URL or infer it from an
opaque endpoint returned by an untrusted provider.

### Porto isolation and lifecycle

Use the existing Docker context **`porto`**, explicitly, for every operation.
Do not switch contexts, start/stop the VM/runtime, change host networking, prune
images/volumes, or touch another project.

Porto 1.10 does not faithfully implement Compose `internal: true`, and rejects
per-container `LogConfig`. The fixture therefore does **not** fall back to a
routable bridge:

1. Caddy owns a `network_mode: none` namespace.
2. The database and both server nodes join that namespace and communicate only
   over its loopback interface.
3. Before readiness, the helper checks the actual namespace's interfaces and
   route table, in addition to container ownership/network configuration.
4. A foreground Python helper binds two random **127.0.0.1-only** host sockets.
   It forwards opaque bytes through `docker --context porto exec -i … nc` to
   fixed Caddy ports in the owned namespace. TLS still terminates in Caddy;
   the relay neither decrypts traffic nor bypasses certificate verification.

No container publishes a database, router, admin or metrics port. Only the two
owned host byte relays are reachable. This tests real TLS/Caddy/Autopush/Postgres,
but is not a production network topology or a packet-capture/all-time egress
claim. The helper is an explicit test dependency, not a browser push daemon.

## Prepare and build

Run commands from this repository's root. The fixture uses Python 3.15 for its
hash-locked sender/tool environment, OpenSSL for optional fictional test
certificates, Cargo, and the already configured Porto/Compose runtime.
It never installs into the global Python environment or replaces global Cargo
tools.

```sh
CACHE="$PWD/tools/push_fixture/.work/cache"
mkdir -p "$CACHE/tools" "$CACHE/build" tools/push_fixture/.work/python-build
export TMPDIR="$PWD/tools/push_fixture/.work/python-build"
export PIP_CACHE_DIR="$CACHE/tools/push-fixture-python/pip-cache"

CARGO_TARGET_DIR="$CACHE/build/push-fixture-audit" \
  cargo install cargo-audit --version 0.23.0 --locked --jobs 2 \
  --root "$CACHE/tools/push-fixture-audit-0.23.0"

python3.15 -m venv "$CACHE/tools/push-fixture-python"
PY="$CACHE/tools/push-fixture-python/bin/python"
"$PY" -m pip install --require-hashes --no-deps \
  -r tools/push_fixture/python-build.lock
"$PY" -m pip install --require-hashes --no-build-isolation \
  -r tools/push_fixture/requirements.lock
"$PY" -m pip check
"$PY" -m pip_audit --disable-pip --require-hashes \
  --cache-dir tools/push_fixture/.work/python-audit-cache \
  -r tools/push_fixture/requirements.lock

python3 tools/push_fixture/fixture.py --cache "$CACHE" build
python3 tools/push_fixture/source_checks.py --cache "$CACHE"
```

The build is `linux/arm64`, `--no-cache`, with two Cargo jobs. It verifies
archives and patches, reconstructs the exact patched tree, and runs a freshly
fetched **cargo-audit 0.23.0 `--deny warnings`** over the complete server lockfile:
no advisory ignores, target filters, or stale/offline advisory exception.
An unavailable/failed audit prevents launch. The minimal runtime contains the
two binaries, their enumerated dynamic libraries, certificates and upstream
MPL notice, not Cargo/CMake/Python.

`export_patches.py` is a maintainer-only export tool. It intentionally updates
reviewed pins and is **never** called by prepare/build/start. Review its output
and re-run the real regressions after any downstream change.

### Downstream changes

The patches remove upstream advisory exceptions, refresh JWT/Actix dependencies,
remove unused SQLx backends and the obsolete hostname helper dependency, and
upgrade the APNS dependency's TLS stack even though APNS is inactive. The retry
dependency uses maintained random/timer implementations. Internal Actix HTTP/2
is disabled rather than retaining vulnerable `h2` 0.3; its original WebSocket
and metrics tests remain, using an HTTP/1-only test harness. Caddy handles
external TLS.

A separate Postgres integrity repair uses the millisecond **sort-key cursor**,
ordered batches and the last batch cursor for fetch/ACK garbage collection,
instead of confusing it with the seconds-resolution TTL timestamp. The
independent stored-batch regression must pass; a working WebSocket alone is not
proof of retained delivery.

The original MPL-2.0 archive and notices remain intact; modified dependency
archives retain their MIT notices. The build-only CMake archive is BSD-3-Clause.
These licenses do not select a Stainless license or clear its publication gate.
Do not distribute this image as a production service without the separate
licensing/notices, image/platform security and operational review.

## Initialize TLS and persistent secrets

Supply a real certificate/key valid for `localhost` (and its issuer chain, as
needed for scoped test trust), or explicitly generate a fictional seven-day
test certificate:

```sh
python3 tools/push_fixture/fixture.py init --cert /owned/cert.pem --key /owned/key.pem
# Alternative for fictional local tests only:
python3 tools/push_fixture/fixture.py init --test-certificate
```

These are alternatives, not sequential commands. Initialization refuses an
existing run. It creates private per-deployment random secrets **once**:

- A shared endpoint encryption key ring for both server nodes.
- Separate endpoint API authentication material (required by the tagged server
  even though bridge registration routes are not publicly proxied).
- A database password.

Secrets remain in the ignored `.work/run/secrets/` directory. Generated config
files are readable inside their unprivileged containers, but their host parent
directory is private. Do not copy config, secret files, endpoints or container
inspection dumps into issues, logs or shell arguments.

The actual key-ring setting is `AUTOCONNECT__CRYPTO_KEYS`, a **string containing
a bracketed list**, not the deprecated singular `…__CRYPTO_KEY`. Autoendpoint
uses `AUTOEND__CRYPTO_KEYS`. This fixture supplies the corresponding TOML
`crypto_keys` fields from the same persistent ring.

Both nodes explicitly set `disable_sentry=true`; leaving the tagged default
unchanged enables reporting. Bigtable and reliable-report features are not
built. FCM credentials and APNS channels are empty, the FCM URL is explicitly
non-cloud, StatsD defaults are patched off, and no broadcast provider is set.
Health checks must report both mobile bridges inactive.

`fresh-init.sql` is derived from the tagged schema, but removes **all** upstream
`DROP` statements and refuses a nonempty schema in a transaction. The Postgres
entrypoint runs it only for a new owned data volume. It is not a migration,
repair or reset command. Never run upstream `schema.psql` against existing data.

## Start, verify and stop

In one terminal (or an attached, owned async tool execution):

```sh
python3 tools/push_fixture/fixture.py --cache "$CACHE" serve
```

This remains in the foreground. In another terminal:

```sh
python3 tools/push_fixture/fixture.py ready
python3 tools/push_fixture/fixture.py status
"$PY" tools/push_fixture/protocol_checks.py --timing --report protocol-report-final.json
```

Readiness requires normal scoped TLS, healthy databases/server version and
inactive FCM/APNS—not just open sockets. The certificate is trusted only by the
test client's SSL context. Native browser tests must use an in-process scoped
test verifier. **Never** add it to Keychain/system trust or pass a TLS-ignore
flag. Real clients retain normal TLS and proxy behavior.

The protocol runner uses independent, hash-locked `pywebpush`/`py-vapid`/
`http-ece` senders. It checks encrypted delivery and rejected VAPID assertions,
ACK100/101/102 consumption, unacknowledged redelivery, stored-batch progress,
offline expiry, unregister, re-registration/A-C reconciliation, persistent-key
restart/rotation, fresh-init refusal, backlog reset, and log redaction.
`--timing` waits for the real 300-second control ping and four-second missing-pong
deadline; it does not shorten server settings to manufacture timing evidence.
Focused reruns use `--case NAME`; reports stay under `.work/`.
The tagged JWT validator allows its library's 60-second clock-skew leeway;
the expired-assertion rejection case uses an assertion ten minutes in the past.

By default the runner stops the helper and all owned containers. Use
`--keep-running` only when another identified test consumer still needs them.
Otherwise stop explicitly:

```sh
python3 tools/push_fixture/fixture.py stop
```

Stop is ownership-checked and idempotent. It stops only inspected labelled
container IDs and the run's private control socket. It retains the database,
key ring, source inputs and built image for reproducible follow-up. No global
cleanup, orphan removal, volume deletion or daemon shutdown is performed.

## Verified wire contract

These shapes are checked against the real patched server, not a simulator:

| Message | JSON fields |
| --- | --- |
| Hello response | `messageType:"hello"`, integer `status:200`, string `uaid` containing **32 lowercase hex digits without hyphens**, boolean `use_webpush:true`, object `broadcasts` |
| Register request | `messageType:"register"`, `channelID` as a **lowercase hyphenated 36-character UUID**, `key` as the base64url application-server public key |
| Register response | `messageType:"register"`, canonical hyphenated string `channelID`, integer `status:200`, HTTPS string `pushEndpoint` |
| Encrypted notification | `messageType:"notification"`, canonical hyphenated string `channelID`, **opaque string** `version`, base64url string `data`, object `headers:{"encoding":"aes128gcm"}` |
| Payload-less notification | Only `messageType`, `channelID`, `version`; **both `data` and `headers` are omitted** |

Register rejects simple/unhyphenated or uppercase channel IDs by closing the
socket. Do not copy hello's UAID formatting rule onto channels. `version` is
not a JSON number or a client-parsed timestamp. Modern headers use **`encoding`**,
not `content-encoding`; the salt and sender key are inside the decoded RFC 8188
body.

Re-registering the same channel and VAPID key returns 200 and leaves the original
opaque endpoint usable; a returned endpoint need not be byte-identical.
Re-registering that channel with another VAPID key also returns 200. It creates
an endpoint bound to the new key, but **does not revoke the old endpoint**:
each endpoint continues to enforce its own VAPID key. Clients must enforce
origin/application-key binding locally, not assume the server detects a changed
website key. Unregistering the channel invalidates its endpoints.
`hello.channelIDs` is ignored, not a channel reconciliation/listing API.

## Restart, rotation, retention and recovery

While the foreground helper is running:

```sh
python3 tools/push_fixture/fixture.py restart
python3 tools/push_fixture/fixture.py rotate
```

Restart recreates only this run's containers, in namespace dependency order;
the database volume and all secrets persist. Rotation prepends a new primary
encryption key and **retains every older decryption key** on both nodes. Original
opaque sender endpoints must still work afterward. Removing an old key can
produce sender HTTP410 without an automatic browser-visible protocol signal.
Recovery then requires explicit registration reset/re-subscription, not a claim
that a browser can detect every destructive operator change.

After an intentionally reviewed rebuild, stop the fixture, run `adopt-build`,
then `serve` again. This updates the image/config binding without recreating
keys or resetting data.

Back up the database and complete secret ring together using normal consistent
Postgres backups. Keep private-key material outside public artifacts. Renew
TLS before expiry and explicitly restart/reload the owned proxy; this fixture
does not choose an automatic certificate provider.

`msg_limit=150` is pinned to the tagged default. It counts messages read from
stored backlog: after the threshold is exceeded, the server drops that UAID
and the next hello returns a new one. It is not unlimited offline retention.
Clients must invalidate old subscriptions and websites must subscribe again.
ACK101/102 **consume**, rather than requeue, server messages. Unacknowledged
stored batches block the next batch; local finite retry handling is essential.

Quitting/disabling the browser stops reception: there is no wake-up daemon.
Offline unregister cannot promise remote deletion. Ciphertext/channel metadata
may remain until expiry/cleanup; TTL and ACK are not global erasure guarantees.
The provider sees connection IP/timing/User-Agent, UAID/channel identifiers,
ciphertext sizes and VAPID keys/claims, including identifying sender subjects.
Payload-less Push is supported; Stainless accepts modern `aes128gcm`, not legacy
`aesgcm`.

Because Porto rejects per-container log rotation settings, this bounded fixture
does not claim enforced long-running log retention. Server logging is restricted
and no Caddy access log is enabled; tests scan actual owned logs for credentials,
identifiers and fictional plaintext. A production operator must separately
configure log/storage retention, backups, TLS renewal, rate/abuse limits and
resource monitoring. Never expose this beta fixture publicly by changing its
bind address or bypassing the isolation/readiness checks.

## Evidence boundary

The source/helper tests and sanitized evidence inventory live in
`tools/push_fixture/`. Detailed local reports remain in ignored `.work/`.
To regenerate `evidence.json`, first run `source_checks.py` and the full protocol
command above. Start `serve` again, then run the focused lifecycle check:

```sh
"$PY" tools/push_fixture/protocol_checks.py \
  --case restart_and_key_rotation --case payloadless_and_unregister \
  --report lifecycle-final.json
python3 tools/push_fixture/record_evidence.py --cache "$CACHE"
```

The recorder refuses failed/missing tests, mismatched source/image inputs, audit
exceptions or still-running services. It obtains binary checksums using a
pinned, network-none checksum stage with an empty build context and removes
only that labelled stage image afterward; no installed binary is modified.
No real user data, account or Keychain is used. None of these server/raw-UA checks
proves native Push permission flow, service-worker dispatch/decryption, visible
notifications, browser profile binding/deduplication, installed behavior,
automatic updates, clean-client verification, or Phase 1 release gates.
Those are separate client/release tests and authorizations.
