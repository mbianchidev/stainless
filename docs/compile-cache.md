# Opt-in compile cache

Repeated Chromium compiles can reuse clang results that another output directory, worktree
or APFS-cloned cache already produced. Stainless uses Siso's built-in REAPI action cache in
**cache-only mode**: every action still runs on this Mac, nothing executes remotely, and a
loopback cache server stores compile results. It is off by default and never used for
release packages.

Google's ABFS was evaluated first and is not usable on macOS; see
[#68](https://github.com/mbianchidev/stainless/issues/68).

## Set up

1. Install [bazel-remote](https://github.com/buchgr/bazel-remote) 2.6.4 or newer:

   ```sh
   brew install bazel-remote
   ```

2. Start it on loopback only, with a bounded size. Keep the cache under the Stainless build
   cache so cleanup rules apply. Pin `--http_address` to loopback too: without it,
   bazel-remote also listens for HTTP on every interface.

   ```sh
   mkdir -p ~/Library/Caches/StainlessBuild/reapi
   bazel-remote --dir ~/Library/Caches/StainlessBuild/reapi --max_size 100 \
     --grpc_address 127.0.0.1:9092 --http_address 127.0.0.1:9090 \
     --access_log_level none
   ```

3. Opt in per command:

   ```sh
   STAINLESS_REAPI_ADDRESS=127.0.0.1:9092 ./tools/stainless doctor
   STAINLESS_REAPI_ADDRESS=127.0.0.1:9092 ./tools/stainless compile --jobs 8
   ```

   `doctor` reports the cache mode, the effective clang platform salt and whether the
   server is reachable. `STAINLESS_REAPI_INSTANCE` optionally names the instance
   (letters, digits, `.`, `_`, `-`; default `stainless`).

Without `STAINLESS_REAPI_ADDRESS`, `compile` and `build` behave exactly as before: offline
Siso (`use_remoteexec = false`), `out/Stainless`, the checked-in GN args and `-j`.

## What changes in cache mode

- **Output directory:** `out/StainlessCache`, never `out/Stainless`. Cached objects cannot
  reach packaging through a later offline incremental build. A marker
  (`stainless-reapi-cache.json`) records the address, instance and platform salt there.
- **GN args:** the checked-in `chromium/args.gn` with only `use_remoteexec = true` changed.
- **Generated Siso profile**, written after `gclient runhooks` (the next offline run's hooks
  restore the upstream files):
  - `buildtools/reclient_cfgs/chromium-browser-clang/rewrapper_mac.cfg` gives clang compiles
    the platform `OSFamily=Darwin,Arch=arm64,stainless-cache-schema=1,stainless-xcode=<build>,stainless-sdk=<build>`.
  - `build/config/siso/backend_config/backend.star` returns empty `default`/`large`
    platforms, so Rust, mojo, Python, codegen and link steps stay local and uncached.
  - `build/config/siso/.sisoenv` sets only the REAPI address and instance, with no cloud
    project. A `.sisorc` file is refused because it could override cache flags.
- **Siso flags:** `-local_jobs=<--jobs>`, `-re_exec_enable=false`,
  `-re_cache_enable_read=true`, `-re_cache_enable_write=true`, explicit
  `-reapi_address`, an empty `-reapi_cas_address`, `-reapi_instance`, `-reapi_insecure`
  and an empty `-project`. `--jobs` keeps meaning local parallelism.
- **Environment:** every inherited `SISO_*` and `RBE_*` variable is removed before any
  Chromium tool runs, in both modes, so nothing can redirect the project, binary or CAS
  endpoint.
- **Report:** after each cache-mode compile, Siso's `siso_metrics.json` is summarized as
  hits, cacheable steps, uploads and failed uploads.

`./tools/stainless compile` runs sources, patches, hooks, local services, GN and the browser
compile without the updater, its AppKit checks or a packageable build manifest. It is the
iteration path for both modes. `build` adds those steps and works in cache mode too, but its
output is still `out/StainlessCache` and cannot be packaged.

## Correctness

Siso computes each action digest from the command line, the clang toolchain inputs and the
headers its dependency scanning finds, so a header, flag or GN-arg change is a different
action and misses. The platform salt adds the Xcode and SDK build versions and a schema
number, so a toolchain change also misses even where scanning does not see SDK files.
Results are only uploaded for actions Siso marks pure. Siso's scandeps and depfile handling
define the input set; that is the trust boundary of this cache.

## Trust model

A poisoned cache would inject objects into a build, so treat it as a supply-chain input:

- **Loopback only.** `STAINLESS_REAPI_ADDRESS` must be a loopback IP and port. Hostnames,
  LAN addresses and port 0 are rejected. Run bazel-remote with loopback gRPC and HTTP
  listeners; it has no authentication.
- **Trusted writers only.** Only local builds from this account write results. Do not share
  the cache directory or expose the server. A future LAN or shared cache needs TLS or mTLS
  (Siso `-reapi_tls_*`, bazel-remote `--tls_*`) and read-only access for untrusted
  builders; that is not supported now.
- **Releases stay uncached.** `package` refuses cache mode, an output directory with the
  cache marker and a build manifest that records `reapi_cache`. Release builds come from
  `out/Stainless`, compiled offline, and remain reproducible without the cache.

## Outages

If the server is down when a cache-mode compile starts, the coordinator stops with a message
to start it or unset `STAINLESS_REAPI_ADDRESS`. If it stops mid-build, Siso treats lookups
as misses and uploads as warnings: the build finishes correctly but partly uncached, and the
report warns about failed uploads. Details are in `out/StainlessCache/siso.INFO`.

## Size, cleanup and removal

`--max_size` bounds the cache in GiB; bazel-remote evicts least recently used entries.
To stop it, interrupt the `bazel-remote` process (Ctrl-C or `kill <PID>`). To remove the
cache, stop the server and delete `~/Library/Caches/StainlessBuild/reapi`. To leave cache
mode, unset `STAINLESS_REAPI_ADDRESS`; delete `out/StainlessCache` only when no session
needs it. Follow the repository's cleanup rules: never delete another session's cache.

## Measurements

Measured on 2026-11-21 on an Apple M5 Max (18 cores, 128 GB, macOS 27.2.1, Xcode 27.2),
Chromium 159.0.8154.13 with the 34-patch stack, `--jobs 18`, bazel-remote 2.6.4 on
loopback. An idle test VM ran during part of the cold build, so treat the numbers as
indicative, not as a benchmark.

| Build | Siso time | End to end | Cacheable clang steps |
| --- | --- | --- | --- |
| Offline, fresh `out/Stainless` (today's default) | 57 min 4 s | 60 min | Not cached |
| Cache mode, cold: fresh output, empty cache | 74 min 9 s | 80 min* | 47,161 uploaded, 0 failed |
| Cache mode, warm: fresh output, filled cache | 7 min 45 s | 10 min 40 s | 46,468 of 47,161 hit (98.5%) |
| Cache mode, incremental after a 3-file patch | - | 3 min 18 s | 18 of 21 hit |

\* Includes rebuilding the local-service runtimes for a llama.cpp update.

- The first cache-mode build costs about 30% more than an offline build (digests and
  uploads). Every later build of the same sources in a fresh output directory, worktree
  or APFS clone is about 7 times faster.
- The cache used 4.7 GiB for one complete browser.
- 691 of the 693 warm misses were V8 compiles. `v8/src/objects/instance-types-gen.h`
  includes `metagen/instance-types.h` under a macro, and Siso's include scanner recorded
  that generated header as an input in one build but not the other. Misses compile
  locally, so this only costs hits.

Correctness checks on the same sources:

- All 47,409 objects from the warm cache-hit build were byte-identical to the offline
  build, and so was the linked `Stainless Framework` binary.
- Changing a header missed; restoring it hit the original digest again. Changing the SDK
  salt missed; restoring it hit. A fresh output directory with the same args hit, and one
  with `symbol_level = 1` missed. An arg that does not change an object's command
  (`dcheck_always_on` for double-conversion) correctly hit.
- The cache-built app passed the full browser smoke in an isolated macOS VM.
- Stopping the server mid-build: 744 hits before the stop, 766 failed uploads afterwards,
  the [outage warning](#outages), a successful build and the same `Stainless Framework` hash.
- With the server stopped, `doctor` and `compile` fail at startup with the offline hint.
- Without the variable, hooks restore the upstream `.sisoenv` and `backend.star`, remove
  the generated rewrapper config and `out/Stainless/args.gn` matches `chromium/args.gn`.
