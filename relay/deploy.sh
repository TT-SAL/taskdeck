#!/usr/bin/env bash
# Deploys the relay to the owner's own Cloudflare account. For the owner, on the
# owner's machine: cloud sessions, agents and CI never run this (SYNC_PLAN.md §3.4).
# The runbook that goes with it is stage `release`'s (docs/sync/runbooks/).
#
#   bash relay/deploy.sh --name <worker name> --origin https://<worker host>      deploy
#   bash relay/deploy.sh --name <worker name> --open                              set a new create secret
#   bash relay/deploy.sh --name <worker name> --close                             delete the create secret
#   bash relay/deploy.sh --dry-run                                                build the bundle, upload nothing
#
# - <worker name> is a random name of your own (SYNC.md §6.1); it is not written into the repository.
# - TOKEN_KEY is drawn once, on the first deploy of a name, and never again: a new key is a new
#   relay (docs/sync/spec/relay.md §5.3). It is never shown.
# - The create secret is shown once, for the device that creates or seeds the board. Run --close
#   as soon as the board exists.
# Needs: wrangler (the version of relay/package.json), logged in to your account, and openssl.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

name="" origin="" mode="deploy"
while [[ $# -gt 0 ]]; do
    case "$1" in
        --name) name="${2:-}"; shift 2 ;;
        --origin) origin="${2:-}"; shift 2 ;;
        --open|--close) mode="${1#--}"; shift ;;
        --dry-run) mode="dry-run"; shift ;;
        *) echo "deploy.sh: unknown argument $1" >&2; exit 2 ;;
    esac
done
wrangler="${WRANGLER:-wrangler}"
command -v "$wrangler" >/dev/null || { echo "deploy.sh: wrangler is not on PATH (set WRANGLER)" >&2; exit 2; }

if [[ "$mode" == dry-run ]]; then
    out="$(mktemp -d)"
    "$wrangler" deploy --dry-run --outdir "$out" --name taskdeck-relay-dry-run
    ls -l "$out"
    rm -rf "$out"
    exit 0
fi

[[ "$name" =~ ^[a-z0-9][a-z0-9-]{2,62}$ ]] || { echo "deploy.sh: --name <worker name> is needed (lowercase letters, digits, hyphens)" >&2; exit 2; }
if [[ "${CI:-}" == true || "${CLAUDE_CODE_REMOTE:-}" == true ]]; then
    echo "deploy.sh: this is CI or a cloud session; the relay is deployed by its owner only" >&2
    exit 2
fi
command -v openssl >/dev/null || { echo "deploy.sh: openssl is needed" >&2; exit 2; }
has_secret() { "$wrangler" secret list --name "$name" 2>/dev/null | grep -q "\"$1\""; }
new_create_secret() {
    local secret
    secret="$(openssl rand -hex 32)"
    printf '%s' "$secret" | "$wrangler" secret put CREATE_SECRET --name "$name" >/dev/null
    echo "The create secret, shown once. Give it to the device that creates or seeds the board,"
    echo "then run: bash relay/deploy.sh --name $name --close"
    echo "  $secret"
}

case "$mode" in
    close)
        "$wrangler" secret delete CREATE_SECRET --name "$name"
        echo "The create secret is deleted: no board can be created or seeded on $name."
        ;;
    open)
        has_secret TOKEN_KEY || { echo "deploy.sh: $name has no TOKEN_KEY: deploy it first" >&2; exit 1; }
        new_create_secret
        ;;
    deploy)
        [[ "$origin" =~ ^https://[a-z0-9.-]+(:[0-9]+)?$ ]] || { echo "deploy.sh: --origin https://<worker host> is needed (lowercase, no path)" >&2; exit 2; }
        "$wrangler" deploy --name "$name" --var "RELAY_ORIGIN:$origin"
        if has_secret TOKEN_KEY; then
            echo "TOKEN_KEY is already set on $name and stays as it is."
        else
            openssl rand -hex 32 | tr -d '\n' | "$wrangler" secret put TOKEN_KEY --name "$name" >/dev/null
            echo "TOKEN_KEY is set on $name. It is not shown and must never be replaced."
            new_create_secret
        fi
        echo "Deployed. The relay origin is $origin"
        ;;
esac
