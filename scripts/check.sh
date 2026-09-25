#!/usr/bin/env bash
# The local quality gate: the same checks as the required CI jobs in
# .github/workflows/ci.yml, cheapest first. Run it before
# pushing or declaring a change done:
#
#   ./scripts/check.sh          fmt, clippy, build, offline tests, rustdoc
#   ./scripts/check.sh --msrv   also type-check on the declared minimum Rust
#   ./scripts/check.sh --cloud  require the Postgres tests (needs
#                               JOBHUNT_TEST_DATABASE_URL; see CONTRIBUTING.md)
#
# Stops at the first failing check and exits non-zero, naming the check.
# Needs only rustup/cargo (plus rustfmt and clippy components). Live source
# tests are never run here; see CONTRIBUTING.md.

set -euo pipefail

usage() {
    sed -n '2,13s/^# \{0,1\}//p' "$0"
}

msrv=false
for arg in "$@"; do
    case "$arg" in
        --msrv) msrv=true ;;
        --cloud)
            if [[ -z "${JOBHUNT_TEST_DATABASE_URL:-}" ]]; then
                echo "--cloud needs JOBHUNT_TEST_DATABASE_URL (a Postgres server)" >&2
                exit 2
            fi
            export JOBHUNT_REQUIRE_POSTGRES=1
            ;;
        -h | --help) usage; exit 0 ;;
        *) echo "unknown argument: $arg" >&2; usage >&2; exit 2 ;;
    esac
done

cd "$(dirname "${BASH_SOURCE[0]}")/.."

if [[ -t 1 ]]; then
    bold=$'\e[1m' red=$'\e[31m' green=$'\e[32m' reset=$'\e[0m'
else
    bold='' red='' green='' reset=''
fi

current=''
started=$SECONDS
report_failure() {
    local status=$?
    if [[ $status -ne 0 && -n "$current" ]]; then
        echo "${red}${bold}FAILED: ${current}${reset}" >&2
    fi
}
trap report_failure EXIT

step() {
    current=$1
    shift
    echo "${bold}==> ${current}${reset}"
    echo "    $*"
    local t=$SECONDS
    "$@"
    echo "${green}    ok${reset} ($((SECONDS - t))s)"
}

# The offline suite must not reach the internet: route any attempt to an
# unreachable proxy (local mock servers are exempt), exactly as CI does.
offline() {
    HTTP_PROXY=http://127.0.0.1:9 HTTPS_PROXY=http://127.0.0.1:9 \
        ALL_PROXY=http://127.0.0.1:9 NO_PROXY=localhost,127.0.0.1,::1 \
        http_proxy=http://127.0.0.1:9 https_proxy=http://127.0.0.1:9 \
        all_proxy=http://127.0.0.1:9 no_proxy=localhost,127.0.0.1,::1 \
        "$@"
}

step "formatting" cargo fmt --all --check
step "clippy" cargo clippy --workspace --all-targets --locked -- -D warnings
step "build" cargo build --workspace --all-targets --locked
step "unit tests" offline cargo test --workspace --locked --no-fail-fast --lib --bins
step "integration tests" offline cargo test --workspace --locked --no-fail-fast --test '*'
step "doc tests" offline cargo test --workspace --locked --no-fail-fast --doc
step "rustdoc" env RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked

if $msrv; then
    version=$(sed -n 's/^rust-version = "\(.*\)"$/\1/p' Cargo.toml)
    if [[ -z "$version" ]]; then
        current="MSRV (no rust-version in Cargo.toml)"
        exit 1
    fi
    if ! rustup run "$version" rustc --version >/dev/null 2>&1; then
        current="MSRV (Rust $version is not installed: rustup toolchain install $version --profile minimal)"
        exit 1
    fi
    step "MSRV (Rust $version)" cargo "+$version" check --workspace --all-targets --locked
fi

current=''
echo "${green}${bold}All checks passed${reset} ($((SECONDS - started))s)"
