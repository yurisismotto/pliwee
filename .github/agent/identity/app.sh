#!/usr/bin/env bash
# app.sh — create the pliwee-worker GitHub App and mint its tokens, for the
# owner, by hand. Nothing here runs in a workflow; the workflows mint their
# tokens with actions/create-github-app-token.
#
#   app.sh form OUT.html     a page that posts the manifest to GitHub's "create
#                            an App from a manifest" flow; open it, confirm
#   app.sh convert CODE      exchange the code GitHub redirected to for the App:
#                            the private key goes to $PLIWEE_APP_KEY (0600) and
#                            is never printed; prints id, slug, client id
#   app.sh token FILE        mint a one-hour installation token for this
#                            repository into FILE (0600), for identity-certify.sh
#
# Environment: PLIWEE_REPO (default yurisismotto/pliwee),
# PLIWEE_APP_KEY (default ~/.config/pliwee-worker-app/private-key.pem),
# PLIWEE_APP_CLIENT_ID (for `token`).
#
# The private key never enters the repository, a log, the sandbox, or Claude.
# Its home in the workflows is the `agent-worker` environment's secret
# PLIWEE_WORKER_APP_PRIVATE_KEY. Delete the local copy once that is set and the
# certification has run: `shred -u "$PLIWEE_APP_KEY"`.

set -euo pipefail
HERE="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_SLUG="${PLIWEE_REPO:-yurisismotto/pliwee}"
KEY="${PLIWEE_APP_KEY:-$HOME/.config/pliwee-worker-app/private-key.pem}"
die() { printf 'app.sh: %s\n' "$*" >&2; exit 1; }
b64url() { openssl base64 -A | tr '+/' '-_' | tr -d '='; }

case "${1:-}" in
form)
    out="${2:?usage: app.sh form OUT.html}"
    m="$(jq -c . "$HERE/pliwee-worker-app.manifest.json")"
    m_html="$(sed 's/&/\&amp;/g; s/"/\&quot;/g; s/</\&lt;/g' <<<"$m")"
    cat > "$out" <<EOF
<!doctype html><meta charset="utf-8"><title>Create pliwee-worker</title>
<form action="https://github.com/settings/apps/new" method="post">
<p>This creates the <b>pliwee-worker</b> GitHub App on your account, with exactly the permissions in
<code>.github/agent/identity/pliwee-worker-app.manifest.json</code>. GitHub shows them before you confirm.</p>
<input type="hidden" name="manifest" value="$m_html">
<button type="submit">Create pliwee-worker on GitHub</button></form>
EOF
    echo "open $out in a browser signed in as the owner; after confirming, GitHub redirects to"
    echo "https://github.com/$REPO_SLUG?code=… — run: $0 convert <that code> (it expires in one hour)" ;;
convert)
    code="${2:?usage: app.sh convert CODE}"
    [[ "$code" =~ ^[A-Za-z0-9]+$ ]] || die "that is not a manifest code"
    [ ! -e "$KEY" ] || die "$KEY already exists; refusing to overwrite a key"
    mkdir -p "$(dirname "$KEY")"; chmod 700 "$(dirname "$KEY")"
    umask 077
    resp="$(mktemp)"; trap 'shred -u "$resp" 2>/dev/null || rm -f "$resp"' EXIT
    curl -fsS -X POST -H 'Accept: application/vnd.github+json' "https://api.github.com/app-manifests/$code/conversions" > "$resp" \
        || die "the conversion failed (an expired or used code?)"
    jq -r .pem "$resp" > "$KEY"; chmod 600 "$KEY"
    [ -s "$KEY" ] && grep -q 'PRIVATE KEY' "$KEY" || die "no private key in the response"
    jq -r '"app id:     \(.id)\nslug:       \(.slug)\nclient id:  \(.client_id)\nbot login:  \(.slug)[bot]\nowner:      \(.owner.login)\npermissions: \(.permissions | tojson)"' "$resp"
    echo "private key: $KEY (0600, not printed)" ;;
token)
    out="${2:?usage: app.sh token FILE}"; cid="${PLIWEE_APP_CLIENT_ID:?set PLIWEE_APP_CLIENT_ID}"
    [ -r "$KEY" ] || die "no readable key at $KEY"
    now="$(date +%s)"
    h="$(printf '{"alg":"RS256","typ":"JWT"}' | b64url)"
    p="$(printf '{"iat":%d,"exp":%d,"iss":"%s"}' "$((now - 60))" "$((now + 540))" "$cid" | b64url)"
    sig="$(printf '%s.%s' "$h" "$p" | openssl dgst -sha256 -sign "$KEY" -binary | b64url)"
    jwt="$h.$p.$sig"
    inst="$(curl -fsS -H "Authorization: Bearer $jwt" -H 'Accept: application/vnd.github+json' \
        "https://api.github.com/repos/$REPO_SLUG/installation" | jq -r .id)" || die "the App is not installed on $REPO_SLUG"
    umask 077
    curl -fsS -X POST -H "Authorization: Bearer $jwt" -H 'Accept: application/vnd.github+json' \
        "https://api.github.com/app/installations/$inst/access_tokens" \
        -d "$(jq -cn --arg r "${REPO_SLUG#*/}" '{repositories:[$r]}')" | jq -r .token > "$out"
    chmod 600 "$out"; [ -s "$out" ] || die "no token minted"
    echo "installation $inst: a one-hour token is in $out (0600, not printed)" ;;
*) sed -n '2,22p' "$0" | sed 's/^# \{0,1\}//' >&2; exit 1 ;;
esac
