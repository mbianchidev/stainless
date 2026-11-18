#!/usr/bin/env bash
# Maintainer release entrypoint for Stainless. Lifecycle, recovery and CI: docs/releasing.md
set -euo pipefail
export GIT_PAGER=cat GH_PAGER=cat PAGER=cat GIT_EDITOR=true GIT_TERMINAL_PROMPT=0 GH_PROMPT_DISABLED=1

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
HELPER="$ROOT/scripts/release_helper.py"
MANIFEST=chromium/manifest.json
WORKFLOW=release.yml

usage() {
    cat <<'EOF'
Usage: ./scripts/release.sh [options] VERSION

Release Stainless VERSION (MAJOR.MINOR.PATCH-preview.N, optional leading "v"):

  1. Validate the version, tools, GitHub auth, remote and existing release state.
  2. If the release base does not carry VERSION yet, open a "release: VERSION"
     bump PR, watch its checks and merge it through gh's --auto merge policy.
  3. In a temporary worktree at the release commit run security-status,
     cargo audit, test, build and package; the package is kept in dist/VERSION.
     test and package launch AppKit helpers and the full browser smoke, so this
     stage requires --isolated-host (docs/build-macos.md#native-macos-gui-test-isolation).
  4. Stage a draft prerelease (./tools/stainless publish) and push the
     annotated stainless-preview-vVERSION tag.
  5. Watch the tag-triggered release workflow that re-verifies and publishes.
  6. Verify the published release, tag and asset digests.
  7. Delete Chromium caches of other revisions, dist packages older than VERSION
     and orphaned temporary directories, unless --keep-artifacts is given.

Each long step runs in its own process group. Interrupts (Ctrl-C/SIGTERM) and
processes a step leaves behind are stopped with SIGTERM, then SIGKILL after a
grace period; Ctrl-C twice forces it. Rerun the same VERSION to resume.

Options:
  -n, --dry-run   Show the plan; create no branch, PR, build, tag or release
      --isolated-host
                  Confirm this is a dedicated macOS build host or VM with its own
                  GUI session and no forwarded personal keyboard input
      --keep-artifacts
                  Keep other Chromium revision caches and older dist packages
      --jobs N    Chromium build parallelism, 1-64 (default 8)
      --remote R  Git remote for the release repository (default origin)
      --base B    Release base branch (default main)
  -h, --help      Show this help

Environment: STAINLESS_CACHE_DIR is passed through to ./tools/stainless.
Timeouts in seconds: STAINLESS_RELEASE_CHECKS_TIMEOUT (600),
STAINLESS_RELEASE_MERGE_TIMEOUT (1800), STAINLESS_RELEASE_RUN_TIMEOUT (600),
STAINLESS_RELEASE_STOP_GRACE (30); STAINLESS_RELEASE_POLL_INTERVAL (10).
EOF
}

log() { printf '==> %s\n' "$*" >&2; }
warn() { printf 'warning: %s\n' "$*" >&2; }
die() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

DRY_RUN=0
ISOLATED=0
KEEP_ARTIFACTS=0
JOBS=8
REMOTE=origin
BASE=main
REQUESTED=""
WORK=""
STATE_DIR=""
JOURNAL=""
LOCKED=0
CHILD=""
SIGNAL=""
JOB_PIDS=()
OWNED_WORKTREES=()
REPOSITORY=""
BASE_SHA=""
RC=""
PR_URL=""
PR_HEAD=""
PKG=""
RUN_URL=""
RELEASE_URL=""
PUSHED_TAG=0

positive() {
    [[ $2 =~ ^[1-9][0-9]{0,5}$ ]] || die "$1 must be a positive integer"
}

parse_args() {
    while (($#)); do
        case $1 in
        -h | --help)
            usage
            exit 0
            ;;
        -n | --dry-run) DRY_RUN=1 ;;
        --isolated-host) ISOLATED=1 ;;
        --keep-artifacts) KEEP_ARTIFACTS=1 ;;
        --jobs | --remote | --base)
            (($# >= 2)) || die "$1 needs a value"
            case $1 in
            --jobs) JOBS=$2 ;;
            --remote) REMOTE=$2 ;;
            --base) BASE=$2 ;;
            esac
            shift
            ;;
        --jobs=*) JOBS=${1#*=} ;;
        --remote=*) REMOTE=${1#*=} ;;
        --base=*) BASE=${1#*=} ;;
        -*) die "Unknown option $1; see --help" ;;
        *)
            [[ -z $REQUESTED ]] || die "Expected exactly one VERSION; see --help"
            REQUESTED=$1
            ;;
        esac
        shift
    done
    [[ -n $REQUESTED ]] || {
        usage >&2
        exit 2
    }
    if [[ ! $JOBS =~ ^[1-9][0-9]?$ ]] || ((JOBS > 64)); then
        die "--jobs must be between 1 and 64"
    fi
    [[ $REMOTE =~ ^[A-Za-z0-9._-]+$ ]] || die "Invalid remote name: $REMOTE"
    git check-ref-format --branch "$BASE" >/dev/null 2>&1 || die "Invalid base branch: $BASE"
    CHECKS_TIMEOUT=${STAINLESS_RELEASE_CHECKS_TIMEOUT:-600}
    MERGE_TIMEOUT=${STAINLESS_RELEASE_MERGE_TIMEOUT:-1800}
    RUN_TIMEOUT=${STAINLESS_RELEASE_RUN_TIMEOUT:-600}
    POLL=${STAINLESS_RELEASE_POLL_INTERVAL:-10}
    STOP_GRACE=${STAINLESS_RELEASE_STOP_GRACE:-30}
    CACHE_DIR=${STAINLESS_CACHE_DIR:-$HOME/Library/Caches/StainlessBuild}
    [[ $CACHE_DIR == /* ]] || die "STAINLESS_CACHE_DIR must be absolute"
    positive STAINLESS_RELEASE_STOP_GRACE "$STOP_GRACE"
    positive STAINLESS_RELEASE_CHECKS_TIMEOUT "$CHECKS_TIMEOUT"
    positive STAINLESS_RELEASE_MERGE_TIMEOUT "$MERGE_TIMEOUT"
    positive STAINLESS_RELEASE_RUN_TIMEOUT "$RUN_TIMEOUT"
    positive STAINLESS_RELEASE_POLL_INTERVAL "$POLL"
}

on_signal() {
    if [[ -n $SIGNAL && -n $CHILD ]]; then
        warn "Forcing the running step to stop"
        signal_job KILL
        return
    fi
    SIGNAL=$1
    if [[ -n $CHILD ]]; then
        log "Stopping the running step (press Ctrl-C again to force it)"
        signal_job TERM
    fi
}

# The running step's process group, its leader's descendants (some tools start their own
# groups) and every process already signaled during this step.
job_members() {
    [[ -n $CHILD ]] || return 0
    {
        pgrep -g "$CHILD" 2>/dev/null || true
        ps -A -o pid= -o ppid= 2>/dev/null | awk -v root="$CHILD" '
            { parent[$1] = $2 }
            END {
                found[root] = 1
                do {
                    added = 0
                    for (pid in parent)
                        if (!(pid in found) && ((parent[pid]) in found)) { found[pid] = 1; added = 1 }
                } while (added)
                for (pid in found) print pid
            }'
        printf '%s\n' ${JOB_PIDS[@]+"${JOB_PIDS[@]}"}
    } | awk -v self="$$" '$1 ~ /^[0-9]+$/ && $1 != self && !seen[$1]++'
}

alive_job_members() {
    local pids
    pids=$(job_members | paste -sd, -)
    [[ -n $pids ]] || return 0
    ps -o pid= -o stat= -p "$pids" 2>/dev/null | awk '$2 !~ /^Z/ { print $1 }'
}

signal_job() {
    local signal=$1 pid
    [[ -n $CHILD ]] || return 0
    while IFS= read -r pid; do
        JOB_PIDS+=("$pid")
        kill "-$signal" "$pid" 2>/dev/null || true
    done < <(job_members)
    kill "-$signal" -- "-$CHILD" 2>/dev/null || true
}

# SIGTERM, wait up to STOP_GRACE seconds, then SIGKILL whatever remains.
stop_job() {
    local deadline=$((SECONDS + STOP_GRACE)) remaining
    signal_job TERM
    while [[ -n $(alive_job_members) ]] && ((SECONDS < deadline)); do
        sleep 1
    done
    [[ -n $(alive_job_members) ]] || return 0
    warn "Processes ignored SIGTERM for ${STOP_GRACE}s; sending SIGKILL"
    signal_job KILL
    sleep 1
    remaining=$(alive_job_members | paste -sd, -)
    [[ -z $remaining ]] || warn "Could not stop processes $remaining; stop them manually"
}

interrupted() {
    case $SIGNAL in
    INT)
        printf 'error: interrupted\n' >&2
        exit 130
        ;;
    TERM)
        printf 'error: terminated\n' >&2
        exit 143
        ;;
    esac
}

# Long tools run as their own process group (scripts/release_helper.py exec-group): xtask
# has no signal handler, so its smoke helpers and build tools are stopped as a group, both
# on interrupt and when a step leaves processes running.
run_long_in() {
    local directory=$1 status=0 leftovers
    shift
    log "$*"
    JOB_PIDS=()
    (cd -- "$directory" && exec python3 "$HELPER" exec-group "$@") &
    CHILD=$!
    wait "$CHILD" 2>/dev/null || status=$?
    if [[ -n $SIGNAL ]]; then
        stop_job
    else
        leftovers=$(alive_job_members | paste -sd, -)
        if [[ -n $leftovers ]]; then
            warn "The step left processes running; stopping them:"
            ps -o pid= -o command= -p "$leftovers" >&2 2>/dev/null || true
            stop_job
        fi
    fi
    CHILD=""
    interrupted
    return "$status"
}

cleanup() {
    local status=$? worktree
    set +e
    trap - INT TERM
    if [[ -n $CHILD ]]; then
        stop_job
        CHILD=""
    fi
    for worktree in ${OWNED_WORKTREES[@]+"${OWNED_WORKTREES[@]}"}; do
        git -C "$ROOT" worktree remove --force "$worktree" >/dev/null 2>&1 ||
            warn "Could not remove the release worktree $worktree"
    done
    [[ -z $WORK || ! -d $WORK ]] || rm -rf -- "$WORK"
    git -C "$ROOT" worktree prune >/dev/null 2>&1
    if ((LOCKED)); then
        : >"$JOURNAL"
        rm -rf -- "$STATE_DIR/lock"
    fi
    if ((status != 0 && DRY_RUN == 0)) && [[ -n ${PLAN_VERSION:-} ]]; then
        printf '\nRelease %s stopped (exit %s). Recorded state:\n' "$PLAN_VERSION" "$status" >&2
        [[ -z $PR_URL ]] || printf '  bump PR:          %s\n' "$PR_URL" >&2
        [[ -z $RC ]] || printf '  release commit:   %s\n' "$RC" >&2
        [[ -z $PKG || ! -d $PKG ]] || printf '  local package:    %s\n' "$PKG" >&2
        [[ -z $RUN_URL ]] || printf '  release workflow: %s\n' "$RUN_URL" >&2
        printf 'Tracked files in your checkout were not modified. Fix the cause, then rerun:\n' >&2
        printf '  ./scripts/release.sh %s\n' "$PLAN_VERSION" >&2
    fi
    exit "$status"
}

require_tools() {
    local tool
    for tool in git gh python3 awk mktemp; do
        command -v "$tool" >/dev/null || die "$tool is required"
    done
    if ((!DRY_RUN)); then
        cargo audit --version >/dev/null 2>&1 ||
            die "cargo-audit is required: cargo install cargo-audit --locked"
    fi
    gh auth status --hostname github.com >/dev/null 2>&1 ||
        die "gh is not authenticated for github.com; run gh auth login"
}

github_repository() {
    local path
    case $1 in
    git@github.com:*) path=${1#git@github.com:} ;;
    ssh://git@github.com/*) path=${1#ssh://git@github.com/} ;;
    https://github.com/*) path=${1#https://github.com/} ;;
    *) return 1 ;;
    esac
    path=${path%/}
    path=${path%.git}
    [[ $path =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] || return 1
    printf '%s\n' "$path" | tr '[:upper:]' '[:lower:]'
}

check_remote() {
    local expected fetch push pushes
    expected=$(sed -n 's|^pub const REPOSITORY: &str = "\([A-Za-z0-9_.-]*/[A-Za-z0-9_.-]*\)";$|\1|p' \
        "$ROOT/components/release/src/lib.rs")
    [[ -n $expected ]] || die "Cannot read the release repository from components/release/src/lib.rs"
    REPOSITORY=$expected
    expected=$(printf '%s\n' "$expected" | tr '[:upper:]' '[:lower:]')
    fetch=$(git -C "$ROOT" remote get-url "$REMOTE" 2>/dev/null) || die "Unknown git remote $REMOTE"
    pushes=$(git -C "$ROOT" remote get-url --push --all "$REMOTE")
    [[ $(grep -c . <<<"$pushes") == 1 ]] || die "Remote $REMOTE must have exactly one push URL"
    push=$pushes
    [[ $(github_repository "$fetch" || true) == "$expected" ]] ||
        die "Remote $REMOTE fetches from $fetch, not github.com/$REPOSITORY"
    [[ $(github_repository "$push" || true) == "$expected" ]] ||
        die "Remote $REMOTE pushes to $push, not github.com/$REPOSITORY"
}

fetch_base() {
    git -C "$ROOT" fetch --quiet --prune "$REMOTE" "+refs/heads/$BASE:refs/remotes/$REMOTE/$BASE" ||
        die "Cannot fetch $BASE from $REMOTE"
    BASE_SHA=$(git -C "$ROOT" rev-parse --verify "refs/remotes/$REMOTE/$BASE^{commit}")
}

check_script_freshness() {
    local name
    for name in release.sh release_helper.py; do
        git -C "$ROOT" show "$BASE_SHA:scripts/$name" 2>/dev/null | cmp -s - "$ROOT/scripts/$name" && continue
        if ((DRY_RUN)); then
            warn "scripts/$name differs from $REMOTE/$BASE; a real release requires the base version"
        else
            die "Run the release script from an up-to-date $BASE checkout (git pull --ff-only)"
        fi
    done
}

# Loads validated KEY=VALUE helper output into PLAN_<KEY> variables without eval.
load_values() {
    local prefix=$1 line key value
    while IFS= read -r line; do
        [[ -n $line ]] || continue
        key=${line%%=*}
        value=${line#*=}
        [[ $key =~ ^[A-Z_]+$ && $value =~ ^[A-Za-z0-9._+-]*$ ]] || die "Unexpected helper output: $line"
        printf -v "${prefix}_$key" '%s' "$value"
    done
}

plan_release() {
    local output
    gh release list --repo "$REPOSITORY" --limit 1000 --json tagName,isDraft \
        --jq '.[] | [.tagName, (.isDraft | tostring)] | @tsv' >"$WORK/releases.tsv" ||
        die "Cannot list releases of $REPOSITORY"
    git -C "$ROOT" show "$BASE_SHA:$MANIFEST" >"$WORK/base-manifest.json"
    output=$(python3 "$HELPER" plan "$REQUESTED" --manifest "$WORK/base-manifest.json" \
        --releases "$WORK/releases.tsv") || exit $?
    load_values PLAN <<<"$output"
    TAG=$PLAN_TAG
    BRANCH="release/$PLAN_VERSION"
    PKG="$ROOT/dist/$PLAN_VERSION"
}

remote_tag_commit() {
    local refs
    refs=$(git -C "$ROOT" ls-remote --tags "$REMOTE") || die "Cannot read tags from $REMOTE"
    awk -v ref="refs/tags/$TAG" '$2 == ref { plain = $1 } $2 == ref "^{}" { peeled = $1 }
        END { print (peeled != "" ? peeled : plain) }' <<<"$refs"
}

remote_branch_commit() {
    local refs
    refs=$(git -C "$ROOT" ls-remote --heads "$REMOTE" "refs/heads/$BRANCH") ||
        die "Cannot read branches from $REMOTE"
    awk -v ref="refs/heads/$BRANCH" '$2 == ref { print $1 }' <<<"$refs"
}

bump_pull_requests() {
    gh pr list --repo "$REPOSITORY" --head "$BRANCH" --base "$BASE" --state all \
        --json url,state,headRefOid,isCrossRepository,mergeCommit \
        --jq '.[] | [.url, .state, .headRefOid, (.isCrossRepository | tostring),
            (.mergeCommit.oid // "")] | @tsv' || die "Cannot list pull requests for $BRANCH"
}

draft_target() {
    gh release view "$TAG" --repo "$REPOSITORY" --json isDraft,targetCommitish \
        --jq '[(.isDraft | tostring), .targetCommitish] | @tsv' || die "Cannot read release $TAG"
}

verify_commit_version() {
    local commit=$1 output
    git -C "$ROOT" merge-base --is-ancestor "$commit" "$BASE_SHA" 2>/dev/null ||
        die "$commit is not part of $REMOTE/$BASE"
    output=$(git -C "$ROOT" show "$commit:$MANIFEST" | python3 "$HELPER" manifest -) || exit $?
    load_values COMMIT <<<"$output"
    [[ $COMMIT_VERSION == "$PLAN_VERSION" && $COMMIT_BUILD == "$PLAN_BUILD" ]] ||
        die "$commit carries $COMMIT_VERSION (build $COMMIT_BUILD), not $PLAN_VERSION (build $PLAN_BUILD)"
}

add_owned_worktree() {
    local path=$1 commit=$2
    printf 'worktree\t%s\n' "$path" >>"$JOURNAL"
    git -C "$ROOT" worktree add --quiet --detach "$path" "$commit" || die "Cannot create worktree $path"
    OWNED_WORKTREES+=("$path")
}

remove_owned_worktree() {
    git -C "$ROOT" worktree remove --force "$1" || die "Cannot remove worktree $1"
    local kept=() worktree
    for worktree in ${OWNED_WORKTREES[@]+"${OWNED_WORKTREES[@]}"}; do
        [[ $worktree == "$1" ]] || kept+=("$worktree")
    done
    OWNED_WORKTREES=(${kept[@]+"${kept[@]}"})
}

acquire_lock() {
    local pid
    STATE_DIR="$(git -C "$ROOT" rev-parse --path-format=absolute --git-common-dir)/stainless-release"
    JOURNAL="$STATE_DIR/journal"
    mkdir -p "$STATE_DIR"
    if ! mkdir "$STATE_DIR/lock" 2>/dev/null; then
        pid=$(cat "$STATE_DIR/lock/pid" 2>/dev/null || true)
        if [[ $pid =~ ^[0-9]+$ ]] && kill -0 "$pid" 2>/dev/null; then
            die "Another release is running (pid $pid)"
        fi
        warn "Removing a stale release lock left by pid ${pid:-unknown}"
        rm -rf -- "$STATE_DIR/lock"
        mkdir "$STATE_DIR/lock" || die "Cannot acquire $STATE_DIR/lock"
    fi
    printf '%s\n' "$$" >"$STATE_DIR/lock/pid"
    LOCKED=1
    if [[ -s $JOURNAL ]]; then
        warn "Cleaning up after an interrupted release attempt"
        local kind path
        while IFS=$'\t' read -r kind path; do
            case $kind in
            worktree)
                if git -C "$ROOT" worktree list --porcelain | grep -Fxq "worktree $path"; then
                    git -C "$ROOT" worktree remove --force "$path" || warn "Could not remove $path"
                fi
                ;;
            workdir)
                [[ $(basename -- "$path") != stainless-release.* || ! -d $path ]] || rm -rf -- "$path"
                ;;
            esac
        done <"$JOURNAL"
        git -C "$ROOT" worktree prune
    fi
    printf 'workdir\t%s\n' "$WORK" >"$JOURNAL"
}

# A resumable bump commit is a single "release: VERSION" commit on the release base that
# changes only the product version/build.
verify_bump_commit() {
    local commit=$1 parents
    git -C "$ROOT" fetch --quiet "$REMOTE" "refs/heads/$BRANCH" || die "Cannot fetch $BRANCH"
    [[ $(git -C "$ROOT" rev-parse FETCH_HEAD) == "$commit" ]] || die "$BRANCH moved while resuming"
    [[ $(git -C "$ROOT" log -1 --format=%s "$commit") == "release: $PLAN_VERSION" ]] ||
        die "$BRANCH is not a release: $PLAN_VERSION commit"
    parents=$(git -C "$ROOT" rev-list --parents -n 1 "$commit" | awk '{ print NF - 1 }')
    [[ $parents == 1 ]] || die "$BRANCH must be a single commit"
    git -C "$ROOT" merge-base --is-ancestor "$commit^" "$BASE_SHA" ||
        die "$BRANCH does not start from $REMOTE/$BASE"
    [[ $(git -C "$ROOT" diff --name-only "$commit^" "$commit") == "$MANIFEST" ]] ||
        die "$BRANCH changes more than $MANIFEST"
    local output
    output=$(git -C "$ROOT" show "$commit:$MANIFEST" | python3 "$HELPER" manifest -) || exit $?
    load_values COMMIT <<<"$output"
    [[ $COMMIT_VERSION == "$PLAN_VERSION" && $COMMIT_BUILD == "$PLAN_BUILD" ]] ||
        die "$BRANCH carries $COMMIT_VERSION build $COMMIT_BUILD, expected build $PLAN_BUILD"
}

create_bump_commit() {
    local worktree="$WORK/bump" remote
    add_owned_worktree "$worktree" "$BASE_SHA"
    python3 "$HELPER" bump --manifest "$worktree/$MANIFEST" --version "$PLAN_VERSION" \
        --build "$PLAN_BUILD" >/dev/null || exit $?
    run_long_in "$worktree" ./tools/stainless security-status || die "security-status failed"
    run_long_in "$worktree" cargo audit --deny warnings || die "cargo audit failed"
    git -C "$worktree" add -- "$MANIFEST"
    [[ $(git -C "$worktree" status --porcelain) == "M  $MANIFEST" ]] ||
        die "Validation changed files other than $MANIFEST"
    git -C "$worktree" -c commit.gpgsign=false commit --quiet --no-gpg-sign -m "release: $PLAN_VERSION"
    PR_HEAD=$(git -C "$worktree" rev-parse HEAD)
    log "Pushing $BRANCH"
    if ! git -C "$worktree" push --quiet "$REMOTE" "$PR_HEAD:refs/heads/$BRANCH"; then
        remote=$(remote_branch_commit)
        [[ $remote == "$PR_HEAD" ]] || die "Pushing $BRANCH failed"
    fi
    remove_owned_worktree "$worktree"
}

create_pull_request() {
    local body
    body="Release bump generated by \`scripts/release.sh\` for **$PLAN_VERSION** (build $PLAN_BUILD).

It changes only \`product.version\` and \`product.build\` in \`$MANIFEST\`. After it merges, the
release script builds, tests and packages the merged commit, stages a draft prerelease and pushes
\`$TAG\`; release CI then verifies and publishes it. See [releasing](docs/releasing.md)."
    log "Opening the bump pull request"
    PR_URL=$(gh pr create --repo "$REPOSITORY" --base "$BASE" --head "$BRANCH" \
        --title "release: $PLAN_VERSION" --body "$body" | tail -n 1) ||
        warn "gh pr create reported an error; checking whether the PR exists"
    if [[ ! $PR_URL =~ ^https://github\.com/[^/]+/[^/]+/pull/[0-9]+$ ]]; then
        sleep "$POLL"
        PR_URL=$(bump_pull_requests | awk -F'\t' '$2 == "OPEN" { print $1; exit }')
    fi
    [[ $PR_URL =~ ^https://github\.com/[^/]+/[^/]+/pull/[0-9]+$ ]] || die "Cannot find the bump PR for $BRANCH"
}

watch_checks() {
    local deadline=$((SECONDS + CHECKS_TIMEOUT)) count results failing head
    log "Waiting for checks on $PR_URL"
    while :; do
        count=$(gh pr checks "$PR_URL" --repo "$REPOSITORY" --json name --jq length 2>/dev/null || true)
        [[ ! $count =~ ^[1-9] ]] || break
        ((SECONDS < deadline)) || die "No checks appeared on $PR_URL within ${CHECKS_TIMEOUT}s"
        sleep "$POLL"
        interrupted
    done
    gh pr checks "$PR_URL" --repo "$REPOSITORY" --watch --fail-fast --interval "$POLL" || true
    interrupted
    results=$(gh pr checks "$PR_URL" --repo "$REPOSITORY" --json name,bucket,link \
        --jq '.[] | [.bucket, .name, .link] | @tsv' || true)
    failing=$(awk -F'\t' '$1 != "pass"' <<<"$results")
    if [[ -z $results || -n $failing ]]; then
        printf '%s\n' "${failing:-no check results}" >&2
        die "Checks did not all pass on $PR_URL; the PR stays open"
    fi
    head=$(gh pr view "$PR_URL" --repo "$REPOSITORY" --json headRefOid --jq .headRefOid)
    [[ $head == "$PR_HEAD" ]] || die "The PR head changed to $head after validation; rerun to revalidate"
}

merge_pull_request() {
    local deadline=$((MERGE_TIMEOUT + SECONDS)) state merge
    log "Merging $PR_URL (gh --auto merges now or once repository requirements pass)"
    gh pr merge "$PR_URL" --repo "$REPOSITORY" --auto --squash --match-head-commit "$PR_HEAD" \
        --subject "release: $PLAN_VERSION" --body "" ||
        die "gh could not merge or enable auto-merge for $PR_URL; it stays open. Merge it per repository policy, then rerun"
    while :; do
        IFS=$'\t' read -r state merge < <(gh pr view "$PR_URL" --repo "$REPOSITORY" \
            --json state,mergeCommit --jq '[.state, (.mergeCommit.oid // "")] | @tsv' || true) || true
        case ${state:-} in
        MERGED) [[ ! ${merge:-} =~ ^[0-9a-f]{40}$ ]] || break ;;
        CLOSED) die "$PR_URL was closed without merging" ;;
        esac
        ((SECONDS < deadline)) || die "$PR_URL did not merge within ${MERGE_TIMEOUT}s; rerun once it merges"
        sleep "$POLL"
        interrupted
    done
    fetch_base
    RC=$merge
}

run_bump() {
    local pulls count state cross merge remote local_branch
    pulls=$(bump_pull_requests)
    count=$(grep -c . <<<"$pulls" || true)
    ((count <= 1)) || die "Several pull requests use $BRANCH; resolve them first"
    remote=$(remote_branch_commit)
    local_branch=$(git -C "$ROOT" rev-parse -q --verify "refs/heads/$BRANCH^{commit}" || true)
    if ((count == 1)); then
        IFS=$'\t' read -r PR_URL state PR_HEAD cross merge <<<"$pulls"
        [[ $cross == false ]] || die "$PR_URL comes from another repository"
        [[ $state == OPEN ]] || die "$PR_URL is $state but $BASE does not carry $PLAN_VERSION; resolve it first"
        [[ $remote == "$PR_HEAD" ]] || die "$PR_URL is open but its branch is missing or moved"
        verify_bump_commit "$PR_HEAD"
        log "Resuming $PR_URL"
    elif [[ -n $remote ]]; then
        PR_HEAD=$remote
        verify_bump_commit "$PR_HEAD"
        create_pull_request
    else
        [[ -z $local_branch ]] || die "Local branch $BRANCH exists without a PR; push or delete it first"
        create_bump_commit
        create_pull_request
    fi
    [[ -z $local_branch || $local_branch == "$PR_HEAD" ]] ||
        die "Local branch $BRANCH conflicts with $PR_URL"
    watch_checks
    merge_pull_request
}

package_revision() {
    local output
    [[ -e $PKG ]] || return 0
    output=$(python3 "$HELPER" package "$PKG") || exit $?
    load_values PACKAGE <<<"$output"
    [[ $PACKAGE_VERSION == "$PLAN_VERSION" && $PACKAGE_TAG == "$TAG" ]] ||
        die "$PKG holds $PACKAGE_VERSION; move it aside"
    printf '%s\n' "$PACKAGE_REVISION"
}

# Tag, draft and local package must agree on one release commit; without them the commit is
# the merged bump PR, or the base head when it already carries VERSION.
resolve_release_commit() {
    local identity source candidate pulls draft target details
    local -a identities=()
    candidate=$(remote_tag_commit)
    identities+=("tag:$candidate")
    if [[ $PLAN_STATE == draft ]]; then
        details=$(draft_target)
        IFS=$'\t' read -r draft target <<<"$details"
        [[ $draft == true && $target =~ ^[0-9a-f]{40}$ ]] ||
            die "Draft $TAG must target a full commit SHA, found ${target:-none}"
        identities+=("draft:$target")
    fi
    candidate=$(package_revision)
    identities+=("package:$candidate")
    for identity in "${identities[@]}"; do
        source=${identity%%:*}
        candidate=${identity#*:}
        [[ -n $candidate ]] || continue
        [[ -z $RC || $RC == "$candidate" ]] || die "The $source for $TAG is $candidate, but other state uses $RC"
        RC=$candidate
    done
    if [[ -n $RC ]]; then
        ((PLAN_BUMP == 0)) || die "Release state exists at $RC but $BASE does not carry $PLAN_VERSION"
        log "Resuming $TAG at $RC"
    elif ((PLAN_BUMP)); then
        run_bump
    else
        pulls=$(bump_pull_requests | awk -F'\t' '$2 == "MERGED" { print $5; exit }')
        RC=${pulls:-$BASE_SHA}
        [[ -z $pulls ]] || log "Using the merged bump commit $RC"
    fi
    verify_commit_version "$RC"
}

build_package() {
    local worktree=$1
    if [[ -e $PKG ]]; then
        log "Reusing the verified local package $PKG"
        return
    fi
    run_long_in "$worktree" ./tools/stainless security-status || die "security-status failed"
    run_long_in "$worktree" cargo audit --deny warnings || die "cargo audit failed"
    # test rebuilds the updater UI, so it must precede the build that records its hash.
    run_long_in "$worktree" ./tools/stainless test || die "tests failed"
    run_long_in "$worktree" ./tools/stainless build --jobs "$JOBS" || die "build failed"
    run_long_in "$worktree" ./tools/stainless package || die "package failed"
    mkdir -p -- "$ROOT/dist"
    [[ ! -e $PKG ]] || die "$PKG appeared during the build"
    mv -- "$worktree/dist/$PLAN_VERSION" "$PKG"
    log "Package kept at $PKG"
}

release_assets() {
    gh release view "$TAG" --repo "$REPOSITORY" --json assets \
        --jq '.assets[] | [.name, (.size | tostring), (.digest // "")] | @tsv' >"$WORK/assets.tsv" ||
        die "Cannot read the assets of $TAG"
}

verify_assets() {
    local sums="$PKG/SHA256SUMS" directory=$PKG
    release_assets
    if [[ ! -f $sums ]]; then
        mkdir -p "$WORK/sums"
        gh release download "$TAG" --repo "$REPOSITORY" --pattern SHA256SUMS --dir "$WORK/sums" ||
            die "Cannot download SHA256SUMS from $TAG"
        sums="$WORK/sums/SHA256SUMS"
        directory=""
    fi
    python3 "$HELPER" verify-assets --sums "$sums" --assets "$WORK/assets.tsv" \
        ${directory:+--directory "$directory"} >/dev/null || return 1
}

stage_draft() {
    local worktree="$WORK/release"
    if [[ $PLAN_STATE == draft ]]; then
        verify_assets ||
            die "Draft $TAG is incomplete or differs from SHA256SUMS; delete only that unpublished draft after inspection, then rerun"
        log "Draft $TAG is complete"
        return
    fi
    add_owned_worktree "$worktree" "$RC"
    build_package "$worktree"
    ! gh release view "$TAG" --repo "$REPOSITORY" >/dev/null 2>&1 ||
        die "A release for $TAG appeared during the build; rerun to reconcile it"
    run_long_in "$worktree" ./tools/stainless publish "$PKG" ||
        die "Staging failed; if a partial draft $TAG exists, delete only that unpublished draft, then rerun"
    remove_owned_worktree "$worktree"
}

push_tag() {
    local remote local_tag
    remote=$(remote_tag_commit)
    if [[ -n $remote ]]; then
        [[ $remote == "$RC" ]] || die "Remote tag $TAG points to $remote, not $RC"
        return
    fi
    local_tag=$(git -C "$ROOT" rev-parse -q --verify "refs/tags/$TAG^{commit}" || true)
    if [[ -z $local_tag ]]; then
        git -C "$ROOT" -c tag.gpgSign=false tag -a "$TAG" -m "release: $PLAN_VERSION" "$RC"
    elif [[ $local_tag != "$RC" ]]; then
        die "Local tag $TAG points to $local_tag, not $RC; delete that unpublished local tag first"
    fi
    log "Pushing $TAG"
    if ! git -C "$ROOT" push --quiet "$REMOTE" "refs/tags/$TAG:refs/tags/$TAG"; then
        remote=$(remote_tag_commit)
        [[ $remote == "$RC" ]] || die "Pushing $TAG failed"
    fi
    PUSHED_TAG=1
}

find_release_run() {
    gh run list --repo "$REPOSITORY" --workflow "$WORKFLOW" --event push --limit 20 \
        --json databaseId,headBranch,headSha,status,conclusion,attempt,url \
        --jq ".[] | select(.headBranch == \"$TAG\" and .headSha == \"$RC\")
            | [(.databaseId | tostring), .status, (if (.conclusion // \"\") == \"\" then \"none\"
                else .conclusion end), (.attempt | tostring), .url] | @tsv" \
        2>/dev/null | head -n 1 || true
}

watch_release_workflow() {
    local deadline=$((SECONDS + RUN_TIMEOUT)) run id status conclusion attempt latest
    log "Waiting for the $WORKFLOW run for $TAG"
    while :; do
        run=$(find_release_run)
        [[ -z $run ]] || break
        ((SECONDS < deadline)) || die "No $WORKFLOW run for $TAG appeared within ${RUN_TIMEOUT}s"
        sleep "$POLL"
        interrupted
    done
    IFS=$'\t' read -r id status conclusion attempt RUN_URL <<<"$run"
    # Only a replay reruns a failure; a run triggered by this invocation's push is reported.
    if [[ $status == completed && $conclusion != success ]] && ((!PUSHED_TAG)); then
        log "Attempt $attempt concluded $conclusion; rerunning its failed jobs once"
        gh run rerun "$id" --repo "$REPOSITORY" --failed || die "Cannot rerun $RUN_URL"
        deadline=$((SECONDS + RUN_TIMEOUT))
        while :; do
            latest=$(gh run view "$id" --repo "$REPOSITORY" --json attempt --jq .attempt 2>/dev/null || true)
            [[ ! $latest =~ ^[0-9]+$ ]] || ((latest <= attempt)) || break
            ((SECONDS < deadline)) || die "The rerun of $RUN_URL did not start within ${RUN_TIMEOUT}s"
            sleep "$POLL"
            interrupted
        done
        status=queued
    fi
    if [[ $status != completed ]]; then
        gh run watch "$id" --repo "$REPOSITORY" --exit-status --interval "$POLL" || true
        interrupted
    fi
    run=$(gh run view "$id" --repo "$REPOSITORY" --json status,conclusion \
        --jq '[.status, .conclusion] | @tsv' || true)
    if [[ $run != $'completed\tsuccess' ]]; then
        gh run view "$id" --repo "$REPOSITORY" --log-failed 2>/dev/null | tail -n 80 >&2 ||
            warn "Failed-step logs are unavailable"
        die "Release workflow did not succeed: $RUN_URL. The draft stays unpublished"
    fi
}

verify_publication() {
    local draft prerelease details
    details=$(gh release view "$TAG" --repo "$REPOSITORY" --json isDraft,isPrerelease,url \
        --jq '[(.isDraft | tostring), (.isPrerelease | tostring), .url] | @tsv') ||
        die "Cannot read release $TAG"
    IFS=$'\t' read -r draft prerelease RELEASE_URL <<<"$details"
    [[ $draft == false && $prerelease == true ]] ||
        die "$TAG is not a published prerelease (draft=$draft, prerelease=$prerelease)"
    [[ $(remote_tag_commit) == "$RC" ]] || die "Remote tag $TAG does not point to $RC"
    verify_assets || die "Published assets of $TAG do not match SHA256SUMS"
}

needs_build() {
    [[ $PLAN_STATE == new && ! -e $PKG ]]
}

# Runs only after the release is verified: removes Chromium caches of other revisions, dist
# packages older than VERSION and orphaned temporary directories. In-use or locked paths are
# kept; published packages remain downloadable from GitHub Releases.
prune_artifacts() {
    local manifest=$1 dry=$2 output
    if ((KEEP_ARTIFACTS)); then
        log "Keeping other Chromium caches and older packages (--keep-artifacts)"
        return
    fi
    output=$(python3 "$HELPER" manifest - <"$manifest") || return 1
    load_values PRUNE <<<"$output"
    ((dry)) || log "Removing Chromium caches other than $PRUNE_CHROMIUM and packages older than $PLAN_VERSION"
    output=$(python3 "$HELPER" prune --cache "$CACHE_DIR" --keep-revision "$PRUNE_CHROMIUM" \
        --dist "$ROOT/dist" --keep-version "$PLAN_VERSION" ${dry:+--dry-run}) || return 1
    load_values PRUNE <<<"$output"
    if ((dry)); then
        printf 'After verifying, would remove %s cache/package/temporary directories (--keep-artifacts keeps them).\n' \
            "$PRUNE_REMOVED"
    else
        log "Removed $PRUNE_REMOVED directories, freed about $PRUNE_FREED_MB MB; kept $PRUNE_SKIPPED in use"
    fi
}

prune_after_release() {
    git -C "$ROOT" show "$RC:$MANIFEST" >"$WORK/release-manifest.json"
    prune_artifacts "$WORK/release-manifest.json" "" ||
        warn "Cleanup did not finish; the release itself is published and verified"
}

dry_run_report() {
    local pulls tag_commit
    pulls=$(bump_pull_requests)
    tag_commit=$(remote_tag_commit)
    printf '\nDry run: no branch, PR, build, tag or release is created.\n'
    printf '  Repository:  %s (%s/%s at %s)\n' "$REPOSITORY" "$REMOTE" "$BASE" "$BASE_SHA"
    printf '  Version:     %s, build %s, tag %s\n' "$PLAN_VERSION" "$PLAN_BUILD" "$TAG"
    printf '  Previous:    %s\n' "${PLAN_PREVIOUS:-none published}"
    printf '  Release:     %s\n' "$PLAN_STATE"
    [[ -z $tag_commit ]] || printf '  Remote tag:  %s\n' "$tag_commit"
    [[ ! -e $PKG ]] || printf '  Package:     %s (reused if it matches the release commit)\n' "$PKG"
    [[ -z $pulls ]] || printf '  Bump PRs:    %s\n' "$(awk -F'\t' '{ print $1 " (" $2 ")" }' <<<"$pulls")"
    if [[ $PLAN_STATE == published ]]; then
        printf 'Would verify the published release, tag and asset digests only.\n'
        prune_artifacts "$WORK/base-manifest.json" 1 || warn "Cannot preview the cleanup"
        return
    fi
    if ((PLAN_BUMP)); then
        printf 'Would open "release: %s" from %s (%s -> %s), watch checks and merge it.\n' \
            "$PLAN_VERSION" "$BRANCH" "$PLAN_CURRENT_VERSION" "$PLAN_VERSION"
    else
        printf '%s/%s already carries %s; no bump PR is needed.\n' "$REMOTE" "$BASE" "$PLAN_VERSION"
    fi
    printf 'Would run security-status, cargo audit, test, build --jobs %s and package at the release\n' "$JOBS"
    if needs_build && ((!ISOLATED)); then
        printf 'The build stage needs --isolated-host on a dedicated macOS build host or VM.\n'
    fi
    printf 'commit, keep dist/%s, stage a draft, push %s, watch %s and verify publication.\n' \
        "$PLAN_VERSION" "$TAG" "$WORKFLOW"
    prune_artifacts "$WORK/base-manifest.json" 1 || warn "Cannot preview the cleanup"
}

main() {
    parse_args "$@"
    trap cleanup EXIT
    trap 'on_signal INT' INT
    trap 'on_signal TERM' TERM
    require_tools
    WORK=$(mktemp -d "${TMPDIR:-/tmp}/stainless-release.XXXXXX")
    WORK=$(cd -- "$WORK" && pwd -P)
    check_remote
    fetch_base
    check_script_freshness
    plan_release
    if ((DRY_RUN)); then
        dry_run_report
        return
    fi
    if needs_build && ((!ISOLATED)); then
        die "Building $TAG runs AppKit helpers and the full browser smoke. Run it on a dedicated macOS build host or VM and pass --isolated-host (docs/build-macos.md#native-macos-gui-test-isolation)"
    fi
    acquire_lock
    if [[ $PLAN_STATE == published ]]; then
        RC=$(remote_tag_commit)
        [[ -n $RC ]] || die "Published release $TAG has no remote tag"
        verify_publication
        log "$TAG is already published and verified: $RELEASE_URL"
        prune_after_release
        return
    fi
    resolve_release_commit
    stage_draft
    push_tag
    watch_release_workflow
    verify_publication
    prune_after_release
    printf '\nReleased %s\n  Release: %s\n  Commit:  %s\n  Package: %s\n' \
        "$TAG" "$RELEASE_URL" "$RC" "$PKG"
    printf 'Clean-host install and installed N-to-N+1 update evidence remain separate gates\n'
    printf '(docs/phase-one.md#release-evidence). Homebrew: see docs/releasing.md#homebrew.\n'
}

main "$@"
