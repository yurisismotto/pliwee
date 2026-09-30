#!/usr/bin/env bash
# release-signing-tests.sh — the negative tests for the signing foundation.
#
# A verifier is only worth having if it says NO. Every case below makes one
# thing wrong and requires `verify-release.sh` to fail; the positive control
# requires it to succeed on an untampered release, so that a verifier which
# rejected everything could not pass this file.
#
# THE TEST KEY
# ------------
# Generated per run, into a temporary GNUPGHOME that is deleted on exit. It is
# never committed, never reused, and its user id says what it is in words that
# cannot be mistaken for a release key:
#
#     OmniBridge TEST KEY -- DO NOT TRUST <test-key@invalid.example>
#
# `.invalid` is reserved by RFC 2606 and can never be a real domain. The key
# has no passphrase, which is correct for an ephemeral key that exists for
# ninety seconds and is the reason the tests need no agent.
#
# Nothing here needs, produces or approaches a production key. That decision is
# the user's and is recorded in docs/audits/release/RELEASE-SIGNING-FOUNDATION-V1.md.

set -uo pipefail
HERE="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd -- "$HERE/../.." && pwd)"
SIGN="$ROOT/packaging/release/sign-release.sh"
VERIFY="$ROOT/packaging/release/verify-release.sh"

PASS=0; FAIL=0; declare -a FAILED=(); declare -a SKIPPED=()
ok()    { PASS=$(( PASS + 1 )); printf 'ok    %s\n' "$*"; }
notok() { FAIL=$(( FAIL + 1 )); FAILED+=("$*"); printf 'not ok  %s\n' "$*"; }
# `n/a` with a reason is evidence; a green tick over nothing is not. A case
# whose precondition this host cannot provide is recorded here, never passed.
skip()  { SKIPPED+=("$*"); printf 'not run  %s\n' "$*"; }
section(){ printf '\n== %s ==\n' "$*"; }
die()   { printf '\nPRECONDITION FAILED: %s\n' "$*" >&2; exit 3; }

command -v gpg >/dev/null 2>&1 || die "gpg is not installed; these tests cannot run"
[ -x "$SIGN" ]   || die "$SIGN is missing or not executable"
[ -x "$VERIFY" ] || die "$VERIFY is missing or not executable"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/pliwee-signing.XXXXXXXX")"
export GNUPGHOME="$WORK/gnupg"
mkdir -p "$GNUPGHOME"; chmod 700 "$GNUPGHOME"
cleanup() {
    gpgconf --kill gpg-agent >/dev/null 2>&1 || true
    rm -rf "$WORK"
}
trap cleanup EXIT

# The tracked-tree private-armour scanner, held in a variable so the shell
# never has to nest one heredoc inside another.
PLIWEE_SCANNER_PY='import os, re, sys
root = sys.argv[1]
hdr = re.compile(r'\''-----BEGIN [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----'\'')
b64 = re.compile(r'\''^[A-Za-z0-9+/=]{40,}$'\'')
for raw in sys.stdin.buffer.read().split(b'\''\0'\''):
    if not raw:
        continue
    rel = raw.decode('\''utf-8'\'', '\''replace'\'')
    fp = os.path.join(root, rel)
    try:
        if os.path.getsize(fp) > 16 * 1024 * 1024:
            continue
        lines = open(fp, encoding='\''utf-8'\'', errors='\''replace'\'').read().splitlines()
    except (OSError, ValueError):
        continue
    for i, line in enumerate(lines):
        if hdr.search(line) and any(b64.match(x.strip()) for x in lines[i + 1:i + 7]):
            print(rel)
            break
'

# The tracked-.pem classifier (owner decision, pre-W8 remediation, 2026-09-25).
# A tracked `.pem` is accepted only when its CONTENT proves it is public: every
# armour block is `CERTIFICATE`, every block parses with `openssl x509`, and
# there is nothing else in the file -- no other label, no text between or
# around the blocks, and no `PRIVATE`/`SECRET` anywhere. Anything else is
# REJECTed with the reason. Prints one line per file: `CERT <path>` or
# `REJECT <path>: <reason>`.
PLIWEE_PEM_CLASSIFIER_PY='import base64, os, re, subprocess, sys, tempfile
root = sys.argv[1]
block = re.compile(r'\''-----BEGIN ([A-Z0-9 ]+)-----\n([A-Za-z0-9+/=\n]+?)\n-----END \1-----'\'')
b64line = re.compile(r'\''^[A-Za-z0-9+/]{1,76}={0,2}$'\'')
def classify(fp):
    try:
        raw = open(fp, "rb").read()
    except OSError as e:
        return "unreadable: %s" % e
    try:
        text = raw.decode("ascii").replace("\r\n", "\n")
    except UnicodeDecodeError:
        return "not ASCII armour"
    if re.search(r'\''PRIVATE|SECRET'\'', text, re.I):
        return "private/secret key-shaped content"
    blocks = list(block.finditer(text))
    if not blocks:
        return "no PEM block"
    rest = block.sub("", text)
    if rest.strip():
        return "content outside the certificate blocks"
    for m in blocks:
        if m.group(1) != "CERTIFICATE":
            return "PEM label %r is not CERTIFICATE" % m.group(1)
        body = m.group(2).split("\n")
        if not all(b64line.match(x) for x in body):
            return "malformed base64 in a CERTIFICATE block"
        der = base64.b64decode("".join(body), validate=True)
        if len(der) < 64 or der[0] != 0x30:
            return "CERTIFICATE block is not a DER SEQUENCE"
        with tempfile.NamedTemporaryFile(suffix=".der") as t:
            t.write(der); t.flush()
            r = subprocess.run(["openssl", "x509", "-inform", "DER", "-in", t.name, "-noout"],
                               capture_output=True)
        if r.returncode != 0:
            return "a CERTIFICATE block does not parse as X.509"
    return None
for raw in sys.stdin.buffer.read().split(b'\''\0'\''):
    if not raw:
        continue
    rel = raw.decode("utf-8", "replace")
    why = classify(os.path.join(root, rel))
    print("CERT %s" % rel if why is None else "REJECT %s: %s" % (rel, why))
'

TEST_UID="OmniBridge TEST KEY -- DO NOT TRUST <test-key@invalid.example>"
WRONG_UID="OmniBridge WRONG TEST KEY -- DO NOT TRUST <wrong-key@invalid.example>"

# ---------------------------------------------------------------------------
section "A test-only keyring"
# ---------------------------------------------------------------------------
gen_key() {
    gpg --batch --quiet --passphrase '' --quick-generate-key "$1" ed25519 sign never >/dev/null 2>&1
}
gen_key "$TEST_UID"  || die "could not generate the test key"
gen_key "$WRONG_UID" || die "could not generate the second (wrong) test key"

FPR="$(gpg --batch --with-colons --list-secret-keys "$TEST_UID"  | awk -F: '/^fpr:/ {print $10; exit}')"
WRONG_FPR="$(gpg --batch --with-colons --list-secret-keys "$WRONG_UID" | awk -F: '/^fpr:/ {print $10; exit}')"
[ -n "$FPR" ] && [ -n "$WRONG_FPR" ] && [ "$FPR" != "$WRONG_FPR" ] \
    || die "the two test keys were not generated distinctly"
ok "two ephemeral test keys exist, $FPR and $WRONG_FPR"

# The user id must be unmistakable. A test key that could be read as a release
# key is the one way this file could do harm.
case "$(gpg --batch --list-keys "$FPR" 2>/dev/null)" in
    *"DO NOT TRUST"*) ok "the test key's user id says DO NOT TRUST in words" ;;
    *) notok "the test key's user id does not carry a DO-NOT-TRUST marker" ;;
esac
case "$TEST_UID" in
    *@invalid.example*) ok "the test key's address is under .invalid, which RFC 2606 reserves" ;;
    *) notok "the test key's address is not under a reserved domain" ;;
esac
[ "$GNUPGHOME" != "${HOME:-}/.gnupg" ] \
    && ok "GNUPGHOME is a temporary directory, not the operator's own keyring" \
    || notok "GNUPGHOME points at the operator's real keyring"

# A keyring holding ONLY the good public key, which is what a user would be
# told to download rather than "whatever your gpg already trusts".
gpg --batch --export "$FPR" > "$WORK/omnibridge-release.gpg" 2>/dev/null
[ -s "$WORK/omnibridge-release.gpg" ] || die "could not export the test public key"
ok "exported a public keyring holding only the expected key"

# ---------------------------------------------------------------------------
section "A release directory shaped like the real one"
# ---------------------------------------------------------------------------
# Same shape as the release job builds: two tarballs at the top, three
# per-distribution directories, an sbom directory, and SHA256SUMS over paths
# rather than bare names because the same filename occurs three times.
mkrelease() {
    local d="$1"
    mkdir -p "$d/fedora44" "$d/ubuntu2404" "$d/ubuntu2604" "$d/debian13" "$d/sbom"
    printf 'source tarball %s\n'  "$RANDOM$RANDOM" > "$d/pliwee-0.0.0-test.tar.gz"
    printf 'vendor tarball %s\n'  "$RANDOM$RANDOM" > "$d/pliwee-0.0.0-test-vendor.tar.xz"
    printf 'rpm %s\n'             "$RANDOM$RANDOM" > "$d/fedora44/pliwee-0.0.0-test.x86_64.rpm"
    printf 'deb u2404 %s\n'       "$RANDOM$RANDOM" > "$d/ubuntu2404/pliwee_0.0.0-test_amd64.deb"
    printf 'deb u2604 %s\n'       "$RANDOM$RANDOM" > "$d/ubuntu2604/pliwee_0.0.0-test_amd64.deb"
    printf 'deb d13 %s\n'         "$RANDOM$RANDOM" > "$d/debian13/pliwee_0.0.0-test_amd64.deb"
    printf '{"sbom":"%s"}\n'      "$RANDOM$RANDOM" > "$d/sbom/pliwee-0.0.0-test.cdx.json"
    ( cd "$d" && find . -type f ! -name 'SHA256SUMS*' -printf '%P\n' | sort | xargs sha256sum > SHA256SUMS )
}
REL="$WORK/release"
mkrelease "$REL"
n="$(grep -c . <"$REL/SHA256SUMS")"
[ "$n" -eq 7 ] || die "the fixture release has $n manifest entries, expected 7"
ok "a fixture release of $n artifacts exists, with SHA256SUMS over paths"

# ---------------------------------------------------------------------------
section "Positive control — an untampered, correctly signed release verifies"
# ---------------------------------------------------------------------------
# Without this, every negative below would also pass on a verifier that simply
# always failed.
"$SIGN" --dir "$REL" --key "$FPR" >"$WORK/sign.log" 2>&1 \
    && ok "sign-release.sh signed the fixture release" \
    || { notok "sign-release.sh failed on a good release: $(tail -1 "$WORK/sign.log")"; }
[ -s "$REL/SHA256SUMS.asc" ] && ok "SHA256SUMS.asc was written" || notok "no SHA256SUMS.asc was written"

if "$VERIFY" --dir "$REL" --keyring "$WORK/omnibridge-release.gpg" --fingerprint "$FPR" >"$WORK/verify.log" 2>&1; then
    ok "POSITIVE CONTROL: verify-release.sh accepts the untampered release"
else
    notok "POSITIVE CONTROL FAILED: a good release did not verify: $(tail -2 "$WORK/verify.log" | tr '\n' ' ')"
fi

# ---------------------------------------------------------------------------
section "Negative tests — each must FAIL verification"
# ---------------------------------------------------------------------------
# refute NAME -- runs the verifier and requires a non-zero exit.
refute() {
    local name="$1"; shift
    if "$@" >"$WORK/neg.log" 2>&1; then
        notok "$name: verification SUCCEEDED and must not have"
    else
        ok "$name: rejected — $(grep -m1 'VERIFICATION FAILED' "$WORK/neg.log" | sed 's/.*VERIFICATION FAILED: //' | tr -s ' ' | head -c 150)"
    fi
}

# 1. a modified artifact
T1="$WORK/t1"; cp -r "$REL" "$T1"
printf 'tampered\n' >> "$T1/ubuntu2404/pliwee_0.0.0-test_amd64.deb"
refute "a modified artifact" "$VERIFY" --dir "$T1" --keyring "$WORK/omnibridge-release.gpg" --fingerprint "$FPR"

# 2. the wrong key
T2="$WORK/t2"; cp -r "$REL" "$T2"
rm -f "$T2/SHA256SUMS.asc"
"$SIGN" --dir "$T2" --key "$WRONG_FPR" >/dev/null 2>&1 || die "could not sign with the wrong key"
refute "a signature by the wrong key" "$VERIFY" --dir "$T2" --keyring "$WORK/omnibridge-release.gpg" --fingerprint "$FPR"

# 2b. the wrong key, against a keyring that does not hold it at all
refute "a signature by a key the keyring does not hold" \
    "$VERIFY" --dir "$T2" --keyring "$WORK/omnibridge-release.gpg"

# 3. a modified checksum manifest — digests edited to match a tampered file,
#    which is exactly what an attacker who can replace files would do
T3="$WORK/t3"; cp -r "$REL" "$T3"
printf 'tampered\n' >> "$T3/fedora44/pliwee-0.0.0-test.x86_64.rpm"
( cd "$T3" && find . -type f ! -name 'SHA256SUMS*' -printf '%P\n' | sort | xargs sha256sum > SHA256SUMS )
( cd "$T3" && sha256sum -c --quiet SHA256SUMS ) \
    && ok "the tampered release is internally consistent, so only the signature can catch it" \
    || notok "the tampered fixture is not internally consistent; the next check would pass for the wrong reason"
refute "a re-generated manifest over tampered files" \
    "$VERIFY" --dir "$T3" --keyring "$WORK/omnibridge-release.gpg" --fingerprint "$FPR"

# 4. a missing signature
T4="$WORK/t4"; cp -r "$REL" "$T4"; rm -f "$T4/SHA256SUMS.asc"
refute "a missing signature" "$VERIFY" --dir "$T4" --keyring "$WORK/omnibridge-release.gpg" --fingerprint "$FPR"
# and the escape hatch is opt-in, prints a warning, and says what it did not check
if out="$("$VERIFY" --dir "$T4" --allow-unsigned 2>&1)"; then
    case "$out" in
        *"CHECKED (UNSIGNED)"*) ok "--allow-unsigned proceeds but reports the result as UNSIGNED" ;;
        *) notok "--allow-unsigned proceeded without labelling the result unsigned" ;;
    esac
    case "$out" in
        *"prove NOTHING about who produced it"*) ok "--allow-unsigned warns what is not being checked" ;;
        *) notok "--allow-unsigned printed no warning" ;;
    esac
else
    notok "--allow-unsigned failed on an unsigned but internally consistent release"
fi

# 5. a missing artifact
T5="$WORK/t5"; cp -r "$REL" "$T5"; rm -f "$T5/sbom/pliwee-0.0.0-test.cdx.json"
refute "an artifact named in the manifest but absent" \
    "$VERIFY" --dir "$T5" --keyring "$WORK/omnibridge-release.gpg" --fingerprint "$FPR"

# 6. a truncated signature
T6="$WORK/t6"; cp -r "$REL" "$T6"
head -c 60 "$REL/SHA256SUMS.asc" > "$T6/SHA256SUMS.asc"
refute "a truncated signature" "$VERIFY" --dir "$T6" --keyring "$WORK/omnibridge-release.gpg" --fingerprint "$FPR"

# 7. an empty signature file — present, so a mere existence check would pass it
T7="$WORK/t7"; cp -r "$REL" "$T7"; : > "$T7/SHA256SUMS.asc"
refute "an empty signature file" "$VERIFY" --dir "$T7" --keyring "$WORK/omnibridge-release.gpg" --fingerprint "$FPR"

# 8. the right key, but a fingerprint the user was told to expect and did not get
refute "a good signature by an unexpected fingerprint" \
    "$VERIFY" --dir "$REL" --keyring "$WORK/omnibridge-release.gpg" --fingerprint "$WRONG_FPR"

# ---------------------------------------------------------------------------
section "The signer must also refuse"
# ---------------------------------------------------------------------------
# 9. it must not sign a manifest that does not describe the files beside it
T9="$WORK/t9"; cp -r "$REL" "$T9"; rm -f "$T9/SHA256SUMS.asc"
printf 'tampered\n' >> "$T9/debian13/pliwee_0.0.0-test_amd64.deb"
if "$SIGN" --dir "$T9" --key "$FPR" >"$WORK/s9.log" 2>&1; then
    notok "sign-release.sh signed a manifest that does not match its files"
else
    ok "sign-release.sh refuses to sign a manifest that does not match its files"
fi
[ -f "$T9/SHA256SUMS.asc" ] \
    && notok "a signature was written despite the refusal" \
    || ok "no signature was written when the signer refused"

# 10. it must not sign a release that would publish private key material
T10="$WORK/t10"; cp -r "$REL" "$T10"; rm -f "$T10/SHA256SUMS.asc"
gpg --batch --pinentry-mode loopback --passphrase '' --export-secret-keys --armor "$FPR" \
    > "$T10/sbom/leaked-secret.asc" 2>/dev/null
[ -s "$T10/sbom/leaked-secret.asc" ] || die "could not stage the leaked-secret fixture"
( cd "$T10" && find . -type f ! -name 'SHA256SUMS*' -printf '%P\n' | sort | xargs sha256sum > SHA256SUMS )
if "$SIGN" --dir "$T10" --key "$FPR" >"$WORK/s10.log" 2>&1; then
    notok "sign-release.sh signed a release containing a PGP PRIVATE KEY BLOCK"
else
    ok "sign-release.sh refuses to sign a release that would publish a private key"
fi

# 11. no secret material may reach the release directory or the logs
section "No signing material leaks"
if grep -rlq -- '-----BEGIN PGP PRIVATE KEY BLOCK-----' "$REL" 2>/dev/null; then
    notok "private key material is present in the signed release directory"
else
    ok "no private key material anywhere in the signed release directory"
fi
secret_sample="$(gpg --batch --pinentry-mode loopback --passphrase '' \
                  --export-secret-keys --armor "$FPR" 2>/dev/null | sed -n '3p')"
[ -n "$secret_sample" ] || die "could not sample the secret key for the leak check"
leaks=0
for log in "$WORK"/*.log; do
    [ -f "$log" ] || continue
    if grep -qF -- "$secret_sample" "$log"; then
        notok "a line of secret key material appears in $(basename "$log")"
        leaks=$(( leaks + 1 ))
    fi
done
[ "$leaks" -eq 0 ] && ok "no secret key material in any script output captured by this run"
# The signer must not print a passphrase either. The strongest form of that
# guarantee is that it never handles one, so the MACHINERY is what is asserted
# absent -- `--passphrase`, a loopback pinentry, a silent `read`, a variable
# holding one. Mentioning the word in the usage text is not only allowed, it is
# how the script tells the operator where the passphrase is supposed to live.
sign_code="$(grep -vE '^\s*#' "$SIGN")"
pp_hits="$(grep -nE -- '--passphrase|--pinentry-mode|read -s|read -rs|PASSPHRASE=|passphrase=' <<<"$sign_code" || true)"
if [ -n "${pp_hits//[[:space:]]/}" ]; then
    notok "sign-release.sh handles passphrase material in code: $(head -1 <<<"$pp_hits" | head -c 100)"
else
    ok "sign-release.sh contains no passphrase-handling code at all; the gpg agent or the secret store holds it"
fi
# Nothing test-only may be committed, and nothing key-shaped either. The keys
# live in $WORK and nowhere else.
#
# NO EXCEPTIONS -- THIS CHECK USED TO HAVE ONE
# --------------------------------------------
# v1.0.0 briefly tracked the armoured PUBLIC release key at
# `packaging/release/omnibridge-release-pubkey.asc`, so that it would have a
# stable raw URL, and this check carried a named exemption for that one path.
# The key is published as a GitHub Release asset instead, the file is gone,
# and the exemption went with it: any tracked `.gpg`/`.asc`/`.key`/`.pem` is a
# failure again, with nothing carved out.
#
# PUBLIC X.509 CERTIFICATES, PROVEN BY CONTENT (2026-09-25, pre-W8 remediation)
# ---------------------------------------------------------------------------
# The owner decided that public X.509 certificates in PEM form may be tracked
# (ADR-0019's two public certificates, android/signing/certs/). That is NOT a
# name exemption: a `.pem` passes only when the classifier above proves every
# block in it is a CERTIFICATE that `openssl x509` parses and that nothing
# else is in the file. A private key saved as `.pem`, a certificate bundled
# with its key, and a key relabelled `CERTIFICATE` are all rejected, and the
# controls below prove each of those rejections on real, ephemeral keys
# before the tree's result is believed. `.gpg`, `.asc` and `.key` are still a
# failure by name, with nothing carved out.
#
# A NAME IS NOT A WARRANT, and that half is kept
# ----------------------------------------------
# The exemption is gone; the lesson that produced it is not. A name rule on its
# own proves nothing about content -- a secret key committed as `notes.txt`
# passes it. So the tracked tree is ALSO read, and no tracked file may carry
# private-key armour whatever it is named. That is strictly more than this
# check established before v1.0.0, and, unlike the exemption, it needs no
# special case to be true.
#
# A here-string, not a pipe: `git ls-files` is long, `grep -q` exits on its
# first match, and under pipefail the SIGPIPE turns a match into a miss -- the
# defect packaging-checks.sh H1 exists to catch, which caught this line.
tracked="$(git -C "$ROOT" ls-files)"
[ -n "${tracked//[[:space:]]/}" ] || die "git ls-files returned nothing; this scan would be vacuous"
key_shaped="$(grep -E '\.(gpg|asc|key)$' <<<"$tracked" || true)"
if [ -n "${key_shaped//[[:space:]]/}" ]; then
    notok "a key-shaped file is committed to the repository: $(head -3 <<<"$key_shaped" | tr '\n' ' ')"
else
    ok "no .gpg/.asc/.key file is committed to the repository"
fi

# --- a tracked .pem must PROVE it is a public certificate --------------------
#
# The classifier is only believed after it has been shown, on this run, to
# accept a real certificate and to reject each private-key shape. The controls
# are generated here, ephemeral, in $WORK; nothing about them is committed.
pem_tracked="$(grep -E '\.pem$' <<<"$tracked" || true)"
n_pem="$(grep -c . <<<"$pem_tracked" || true)"
if ! command -v python3 >/dev/null 2>&1 || ! command -v openssl >/dev/null 2>&1; then
    # Not a skip: without the tools a tracked .pem cannot be proven public,
    # and an unproven .pem is exactly what this check exists to refuse.
    if [ "${n_pem:-0}" -gt 0 ]; then
        notok "python3/openssl absent: $n_pem tracked .pem file(s) could NOT be proven to be public certificates"
    else
        skip "python3/openssl absent: the .pem classifier controls did not run (no .pem is tracked)"
    fi
else
    PEMPY="$WORK/classify-pem.py"
    printf '%s' "$PLIWEE_PEM_CLASSIFIER_PY" > "$PEMPY"
    PC="$WORK/pem-control"; rm -rf "$PC"; mkdir -p "$PC"
    openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -days 1 \
        -subj "/CN=pliwee-test-only.invalid" \
        -keyout "$PC/private-pkcs8.pem" -out "$PC/public-cert.pem" >/dev/null 2>&1 \
        || die "openssl could not make the ephemeral control certificate and key"
    grep -q -- '-----BEGIN PRIVATE KEY-----' "$PC/private-pkcs8.pem" \
        || die "the ephemeral control key is not a PKCS#8 PRIVATE KEY; the controls would test nothing"
    openssl ec -in "$PC/private-pkcs8.pem" -out "$PC/private-ec.pem" >/dev/null 2>&1 \
        || openssl pkey -in "$PC/private-pkcs8.pem" -traditional -out "$PC/private-ec.pem" >/dev/null 2>&1 \
        || die "openssl could not write the traditional EC form of the control key"
    openssl pkcs8 -topk8 -in "$PC/private-pkcs8.pem" -passout pass:control-only \
        -out "$PC/private-encrypted.pem" >/dev/null 2>&1 \
        || die "openssl could not write the encrypted form of the control key"
    cat "$PC/public-cert.pem" "$PC/private-pkcs8.pem" > "$PC/cert-plus-key.pem"
    # A private key's body under a CERTIFICATE label: only the X.509 parse
    # can see through it, so the word PRIVATE is removed from this one.
    { echo '-----BEGIN CERTIFICATE-----'
      sed -e '/-----/d' "$PC/private-pkcs8.pem"
      echo '-----END CERTIFICATE-----'; } > "$PC/key-relabelled.pem"
    { cat "$PC/public-cert.pem"; echo 'trailing note'; } > "$PC/cert-plus-text.pem"
    ctl="$(printf '%s\0' public-cert.pem private-pkcs8.pem private-ec.pem private-encrypted.pem \
                         cert-plus-key.pem key-relabelled.pem cert-plus-text.pem \
           | python3 "$PEMPY" "$PC")"
    [ "$(grep -c . <<<"$ctl")" -eq 7 ] \
        || die "the .pem classifier answered for $(grep -c . <<<"$ctl") of 7 control files; its verdicts mean nothing"
    if grep -qxF 'CERT public-cert.pem' <<<"$ctl"; then
        ok "pem control: a real public X.509 certificate is accepted"
    else
        die "the .pem classifier rejects a real certificate ($(grep public-cert <<<"$ctl")); a FAIL below would be about the classifier"
    fi
    for c in private-pkcs8.pem private-ec.pem private-encrypted.pem cert-plus-key.pem \
             key-relabelled.pem cert-plus-text.pem; do
        if grep -q "^REJECT $c: " <<<"$ctl"; then
            ok "pem control: $c is rejected ($(sed -n "s/^REJECT $c: //p" <<<"$ctl"))"
        else
            notok "pem control: the classifier ACCEPTS $c -- private key material could be committed as .pem"
        fi
    done
    # The armour scanner, too, must see a PEM private key, not only a PGP one.
    ARMOURPY="$WORK/scan-armour.py"
    printf '%s' "$PLIWEE_SCANNER_PY" > "$ARMOURPY"
    pem_armour="$(printf 'private-pkcs8.pem\0private-ec.pem\0private-encrypted.pem\0public-cert.pem\0' \
                  | python3 "$ARMOURPY" "$PC")"
    if [ "$(grep -c . <<<"$pem_armour")" -eq 3 ] && ! grep -qxF 'public-cert.pem' <<<"$pem_armour"; then
        ok "pem control: the tracked-tree armour scanner finds all three PEM private-key forms and not the certificate"
    else
        notok "pem control: the armour scanner found [$(paste -sd' ' <<<"$pem_armour")] of the three PEM private keys"
    fi

    if [ "${n_pem:-0}" -eq 0 ]; then
        ok "no .pem file is committed to the repository"
    else
        verdicts="$(tr '\n' '\0' <<<"$pem_tracked" | python3 "$PEMPY" "$ROOT")"
        [ "$(grep -c . <<<"$verdicts")" -eq "$n_pem" ] \
            || die "the classifier answered for $(grep -c . <<<"$verdicts") of $n_pem tracked .pem file(s)"
        rejected="$(grep '^REJECT ' <<<"$verdicts" || true)"
        if [ -n "${rejected//[[:space:]]/}" ]; then
            notok "a tracked .pem is not a public certificate: $(head -3 <<<"$rejected" | paste -sd' ')"
        else
            ok "every tracked .pem ($n_pem) is a public X.509 certificate and nothing else: $(sed 's/^CERT //' <<<"$verdicts" | paste -sd' ')"
        fi
    fi
fi

# --- and no tracked file carries private key armour, whatever it is called ---
#
# A HEADER IS NOT A KEY. Every guard that looks for leaked key material has to
# CONTAIN the string it looks for, so this file, sign-release.sh,
# release-signing-production-tests.sh and the release workflow all carry
# `-----BEGIN PGP PRIVATE KEY BLOCK-----` as a pattern literal. Matching the
# header alone would report several leaked keys in a tree that has none -- a
# FAIL about the test rather than the product, which is the half of the
# AGENTS.md rule that is easy to miss. A hit therefore requires a long base64
# payload within six lines of the header.
if ! command -v python3 >/dev/null 2>&1; then
    skip "python3 is absent, so the tracked tree was NOT scanned for private key armour"
else
    # The detector proves itself before its silence is worth anything: a
    # scanner that finds nothing has two explanations and only one is good
    # news. The control is a REAL exported secret key -- the ephemeral per-run
    # test identity, never a production one.
    priv_re='-----BEGIN (PGP |OPENSSH |ENCRYPTED |RSA |EC |DSA )?PRIVATE KEY( BLOCK)?-----'
    CANARY_KEY="$WORK/tracked-scan-canary.asc"
    gpg --batch --pinentry-mode loopback --passphrase '' \
        --armor --export-secret-keys "$FPR" > "$CANARY_KEY" 2>/dev/null
    if grep -qE -- "$priv_re" "$CANARY_KEY"; then
        ok "tracked-tree precondition: a REAL armoured private key ($(wc -c <"$CANARY_KEY") bytes) is available as a control"
    else
        die "could not export a real secret key for the control; the verdict below would mean nothing"
    fi

    SCANPY="$WORK/scan-tracked.py"
    printf '%s' "$PLIWEE_SCANNER_PY" > "$SCANPY"

    # The controls run through the SAME scanner, on a scratch tree, so a
    # scanner broken in a way that reports nothing cannot pass this file.
    CTL="$WORK/tracked-scan-control"; rm -rf "$CTL"; mkdir -p "$CTL"
    cp "$CANARY_KEY" "$CTL/planted.asc"
    printf 'guard pattern with no payload: %s\n' \
        '-----BEGIN PGP PRIVATE KEY BLOCK-----' > "$CTL/guard-shaped.sh"
    ctl_hits="$(printf 'planted.asc\0guard-shaped.sh\0' | python3 "$SCANPY" "$CTL")"
    if grep -qxF 'planted.asc' <<<"$ctl_hits"; then
        ok "tracked-scan control: the scanner FINDS a planted armoured private key"
    else
        die "the tracked-tree scanner did not find a key it was handed; its silence would mean nothing"
    fi
    if grep -qxF 'guard-shaped.sh' <<<"$ctl_hits"; then
        notok "the tracked-tree scanner mistakes a guard's own pattern literal for a key"
    else
        ok "tracked-scan control: a header with no payload is NOT mistaken for a key"
    fi

    n_tracked="$(grep -c . <<<"$tracked" || true)"
    [ "${n_tracked:-0}" -ge 100 ] \
        || die "only ${n_tracked:-0} tracked file(s); the scan below would be vacuous"
    armoured="$(git -C "$ROOT" ls-files -z | python3 "$SCANPY" "$ROOT")"
    if [ -n "${armoured//[[:space:]]/}" ]; then
        notok "tracked file(s) carry private key armour: $(head -3 <<<"$armoured" | tr '\n' ' ')"
    else
        ok "no tracked file carries private key armour ($n_tracked files scanned)"
    fi
fi

# --- the documented verification recipe keeps its anchor --------------------
#
# The key now lives only on the release page, so what the repository still
# owes the reader is the fingerprint to compare it against and a recipe that
# actually pins it. A recipe that dropped `--fingerprint` would accept any key
# its keyring happens to trust -- the failure SIGN-NEG-03b exists for.
readme_fpr="$(sed -n 's/^primary  *\([0-9A-Fa-f]\{40\}\) *$/\1/p' "$ROOT/README.md" | head -1)"
if [ -n "$readme_fpr" ]; then
    ok "README.md publishes a primary fingerprint to compare against ($readme_fpr)"
else
    notok "README.md publishes no 'primary <fingerprint>' line; the recipe has no anchor"
fi
if grep -q -- '--fingerprint' "$ROOT/README.md"; then
    ok "README.md's verification recipe pins the key with --fingerprint"
else
    notok "README.md's verification recipe does not pass --fingerprint"
fi
# From the first Pliwee release the asset is pliwee-release-pubkey.asc: the
# same key, with the Pliwee UID added (ADR-0020 D6). OmniBridge v1.0.0 keeps
# its own copy under the old name on its own release page.
if grep -qE 'releases/[^ )]*pliwee-release-pubkey\.asc' "$ROOT/README.md"; then
    ok "README.md tells the reader to obtain the key from the release page"
else
    notok "README.md does not say where to obtain the public key"
fi
# And it must not send them back to a repository copy that no longer exists.
if grep -qE 'raw\.githubusercontent\.com.*(pliwee|omnibridge)-release-pubkey' "$ROOT/README.md"; then
    notok "README.md still points at a repository copy of the public key, which is not tracked"
else
    ok "README.md does not point at a repository copy of the public key"
fi

# ---------------------------------------------------------------------------
section "A certify-only master with a signing subkey"
# ---------------------------------------------------------------------------
# The structure RELEASE-SIGNING-FOUNDATION-V1.md recommends, and the one that
# caught a real defect before it shipped: gpg signs with the SUBKEY when asked
# for the master, so a check that compared only the signing key's fingerprint
# rejected the recommended layout, and a --fingerprint check against the
# PUBLISHED primary fingerprint told the user their release was substituted.
#
# What a project publishes is the primary fingerprint. What the signature
# carries is the subkey's. Both must be accepted, and everything else must
# still be refused.
SUB_UID="OmniBridge SUBKEY TEST -- DO NOT TRUST <subkey-test@invalid.example>"
gpg --batch --quiet --passphrase '' --quick-generate-key "$SUB_UID" ed25519 cert never >/dev/null 2>&1     || die "could not generate the certify-only master"
MFPR="$(gpg --batch --with-colons --list-secret-keys "$SUB_UID" | awk -F: '/^fpr:/ {print $10; exit}')"
gpg --batch --quiet --passphrase '' --quick-add-key "$MFPR" ed25519 sign 2y >/dev/null 2>&1     || die "could not add the signing subkey"
SFPR="$(gpg --batch --with-colons --list-keys "$MFPR" | awk -F: '/^sub:/{f=1} /^fpr:/{if(f){print $10; exit}}')"
[ -n "$MFPR" ] && [ -n "$SFPR" ] && [ "$MFPR" != "$SFPR" ]     || die "the master and subkey fingerprints were not produced distinctly"
ok "certify-only master $MFPR with signing subkey $SFPR"

# The master must not itself be able to sign data; that is the whole point.
mcap="$(gpg --batch --with-colons --list-keys "$MFPR" | awk -F: '/^pub:/ {print $12; exit}')"
case "$mcap" in
    *c*) ok "the master's own capability is certify (flags: $mcap)" ;;
    *) notok "the master does not carry a certify capability (flags: $mcap)" ;;
esac
case "$mcap" in
    *s*) notok "the master can sign data itself; it was meant to be certify-only (flags: $mcap)" ;;
    *) ok "the master cannot sign data itself, so a compromised signing subkey does not imply a compromised identity" ;;
esac

# GnuPG 2.1+ writes a revocation certificate at key creation. Recommending
# that file is only honest if it is actually there.
[ -s "$GNUPGHOME/openpgp-revocs.d/$MFPR.rev" ]     && ok "gpg pre-generated a revocation certificate at openpgp-revocs.d/$MFPR.rev ($(wc -c <"$GNUPGHOME/openpgp-revocs.d/$MFPR.rev") bytes)"     || notok "no pre-generated revocation certificate for $MFPR; the documented backup step would point at nothing"

SR="$WORK/subkey-release"
mkrelease "$SR"
gpg --batch --export "$MFPR" > "$WORK/subkey-pub.gpg"

"$SIGN" --dir "$SR" --key "$MFPR" >"$WORK/sign-sub.log" 2>&1     && ok "sign-release.sh signs when given the MASTER fingerprint"     || notok "sign-release.sh failed on a master+subkey key: $(tail -1 "$WORK/sign-sub.log")"
grep -q 'by subkey' "$WORK/sign-sub.log"     && ok "sign-release.sh reports that a subkey made the signature"     || notok "sign-release.sh did not report the subkey relationship"

if "$VERIFY" --dir "$SR" --keyring "$WORK/subkey-pub.gpg" --fingerprint "$MFPR" >"$WORK/v-pri.log" 2>&1; then
    ok "verify-release.sh accepts the PUBLISHED primary fingerprint"
else
    notok "verify-release.sh rejected the primary fingerprint, which is what users are told to check: $(tail -1 "$WORK/v-pri.log")"
fi
if "$VERIFY" --dir "$SR" --keyring "$WORK/subkey-pub.gpg" --fingerprint "$SFPR" >/dev/null 2>&1; then
    ok "verify-release.sh also accepts the signing subkey's own fingerprint"
else
    notok "verify-release.sh rejected the subkey fingerprint that made the signature"
fi
# and it must NOT have become permissive in the process
refute "a master+subkey signature against an unrelated fingerprint"     "$VERIFY" --dir "$SR" --keyring "$WORK/subkey-pub.gpg" --fingerprint "$FPR"
T11="$WORK/t11"; cp -r "$SR" "$T11"
printf 'tampered\n' >> "$T11/ubuntu2404/pliwee_0.0.0-test_amd64.deb"
refute "a modified artifact under a master+subkey signature"     "$VERIFY" --dir "$T11" --keyring "$WORK/subkey-pub.gpg" --fingerprint "$MFPR"

# ---------------------------------------------------------------------------
section "A --keyring that gpg silently ignores"
# ---------------------------------------------------------------------------
# `--no-default-keyring --keyring FILE` is IGNORED whenever gpg runs with
# `use-keyboxd` -- one line in ~/.gnupg/common.conf, and the configuration this
# project's own maintainer workstation (Fedora 44, gpg 2.4.9) ships with. gpg
# prints a Note on stderr, exits 0, and answers out of the user's OWN keyring.
#
# Measured on gpg 2.4.9 before the fix, in both directions:
#
#   FALSE PASS  a release reported VERIFIED against a --keyring that did not
#               contain the signing key at all, because the user's own keyring
#               did -- while the script printed "checking against <file>";
#   FALSE FAIL  a genuine release, correct --keyring, on a keyboxd host that
#               had not imported the key: "This is what a substituted release
#               looks like".
#
# The fix imports the given keyring into a private GNUPGHOME and verifies
# there. The first two checks below are host-independent and encode the fix
# itself; the behavioural ones need keyboxd and say so when they cannot run.

# Comment lines are stripped first: this file and verify-release.sh both
# DISCUSS the flags that lie, and a grep over prose would report the defect
# present forever after it was fixed.
verify_code="$(grep -v '^[[:space:]]*#' "$VERIFY")"
if grep -q -e '--no-default-keyring' <<<"$verify_code"; then
    notok "verify-release.sh still passes --no-default-keyring in code, which keyboxd silently ignores"
else
    ok "verify-release.sh's code does not rely on --no-default-keyring/--keyring, which keyboxd silently ignores"
fi
if grep -q -e '--homedir "$ISOHOME"' <<<"$verify_code"; then
    ok "verify-release.sh verifies inside a private GNUPGHOME it built from --keyring"
else
    notok "verify-release.sh does not isolate --keyring into a private GNUPGHOME"
fi

gpg --batch --export "$WRONG_FPR" > "$WORK/stranger-pub.gpg" 2>/dev/null

KBX="$WORK/kbx-holder"; mkdir -p "$KBX"; chmod 700 "$KBX"
printf 'use-keyboxd\n' > "$KBX/common.conf"
gpg --homedir "$KBX" --batch --quiet --import < "$WORK/subkey-pub.gpg" >/dev/null 2>&1

# Is keyboxd actually in effect here? If this host cannot enable it, the defect
# cannot manifest and the two cases below measure nothing. They are then
# recorded as not executed, with the reason, rather than passing.
kbx_probe="$(gpg --homedir "$KBX" --batch --no-default-keyring --keyring "$WORK/stranger-pub.gpg" --list-keys 2>&1)"
case "$kbx_probe" in
    *use-keyboxd*) KBX_ACTIVE=1 ;;
    *)             KBX_ACTIVE=0 ;;
esac

if [ "$KBX_ACTIVE" = "1" ]; then
    ok "a keyboxd-configured fixture keyring is in effect, so the regression is measurable here"

    # The positive control comes FIRST. Without it, the refutation below would
    # also pass on a verifier that rejects everything.
    if env GNUPGHOME="$KBX" "$VERIFY" --dir "$SR" --keyring "$WORK/subkey-pub.gpg" --fingerprint "$MFPR" >/dev/null 2>&1; then
        ok "positive control: on that keyboxd host, the CORRECT keyring verifies"
    else
        notok "the correct keyring failed on a keyboxd host; the refutation below would be vacuous"
    fi

    refute "FALSE PASS: a keyring without the signer, on a keyboxd host whose own keyring has it" \
        env GNUPGHOME="$KBX" "$VERIFY" --dir "$SR" --keyring "$WORK/stranger-pub.gpg" --fingerprint "$MFPR"

    KBX2="$WORK/kbx-empty"; mkdir -p "$KBX2"; chmod 700 "$KBX2"
    printf 'use-keyboxd\n' > "$KBX2/common.conf"
    if env GNUPGHOME="$KBX2" "$VERIFY" --dir "$SR" --keyring "$WORK/subkey-pub.gpg" --fingerprint "$MFPR" >/dev/null 2>&1; then
        ok "FALSE FAIL: a genuine release verifies on a keyboxd host that never imported the key"
    else
        notok "FALSE FAIL regression: a genuine release was rejected on a keyboxd host"
    fi
    env GNUPGHOME="$KBX2" gpgconf --kill keyboxd >/dev/null 2>&1 || true
else
    skip "keyboxd could not be enabled in a fixture keyring on this host, so the two keyboxd regressions were NOT EXECUTED"
fi
env GNUPGHOME="$KBX" gpgconf --kill keyboxd >/dev/null 2>&1 || true

# A keyring that holds no key must be refused, not treated as a strict check:
# it rejects every signature, which is indistinguishable from catching a bad one.
printf 'this is not a keyring\n' > "$WORK/garbage-keyring.gpg"
refute "a --keyring file that yields no public key" \
    "$VERIFY" --dir "$SR" --keyring "$WORK/garbage-keyring.gpg" --fingerprint "$MFPR"

# A verifier must hold no secret. A keyring carrying one makes every later
# claim about "no secret key present" false.
gpg --batch --pinentry-mode loopback --passphrase '' --export-secret-keys "$MFPR" > "$WORK/secret-keyring.gpg" 2>/dev/null
refute "a --keyring carrying secret key material" \
    "$VERIFY" --dir "$SR" --keyring "$WORK/secret-keyring.gpg" --fingerprint "$MFPR"
rm -f "$WORK/secret-keyring.gpg"

# ---------------------------------------------------------------------------
section "An operational keyring holding only the signing subkey"
# ---------------------------------------------------------------------------
# The custody model in RELEASE-SIGNING-FOUNDATION-V1.md §8.3: the certify-only
# master lives offline, and only the signing subkey's secret is on the machine
# that makes releases. Generating both into ~/.gnupg and leaving them there --
# which §8.6 originally instructed -- does not implement it.
OPS="$WORK/ops-gnupg"; mkdir -p "$OPS"; chmod 700 "$OPS"
gpg --batch --pinentry-mode loopback --passphrase '' --export-secret-subkeys "$MFPR" 2>/dev/null \
  | gpg --homedir "$OPS" --batch --pinentry-mode loopback --passphrase '' --import >/dev/null 2>&1

ops_list="$(gpg --homedir "$OPS" --list-secret-keys 2>/dev/null)"
case "$ops_list" in
    *"sec#"*) ok "the operational keyring lists the primary as 'sec#' — its secret half is absent" ;;
    *)        notok "the operational keyring does not show 'sec#'; the master secret may be present" ;;
esac
case "$ops_list" in
    *ssb*) ok "the operational keyring holds the signing subkey's secret ('ssb')" ;;
    *)     notok "the operational keyring holds no signing secret" ;;
esac

# The listing is a label; the keygrip files are the fact.
ops_pri_grp="$(gpg --homedir "$OPS" --with-colons --with-keygrip --list-keys "$MFPR" | awk -F: '$1=="pub"{f=1} f&&$1=="grp"{print $10; exit}')"
if [ -n "$ops_pri_grp" ] && [ -e "$OPS/private-keys-v1.d/$ops_pri_grp.key" ]; then
    notok "the primary's secret key file $ops_pri_grp.key is present in the operational keyring"
else
    ok "the primary's keygrip has no secret key file in the operational keyring"
fi
ops_nsec="$(find "$OPS/private-keys-v1.d" -type f -name '*.key' 2>/dev/null | wc -l)"
if [ "${ops_nsec:-0}" -eq 1 ]; then
    ok "exactly one secret key file — the signing subkey — in the operational keyring"
else
    notok "$ops_nsec secret key files in the operational keyring (expected exactly 1)"
fi

OSR="$WORK/ops-release"
mkrelease "$OSR"
if env GNUPGHOME="$OPS" "$SIGN" --dir "$OSR" --key "$MFPR" >"$WORK/ops-sign.log" 2>&1; then
    ok "sign-release.sh signs by the PRIMARY fingerprint although the primary's secret is absent"
else
    notok "signing failed with only the subkey secret present: $(tail -1 "$WORK/ops-sign.log")"
fi
if "$VERIFY" --dir "$OSR" --keyring "$WORK/subkey-pub.gpg" --fingerprint "$MFPR" >/dev/null 2>&1; then
    ok "a subkey-only signature verifies against the published primary fingerprint"
else
    notok "a subkey-only signature did not verify against the published primary fingerprint"
fi

# Certification is what the offline master is FOR. If it can be done here, the
# master is not offline.
if env GNUPGHOME="$OPS" gpg --batch --pinentry-mode loopback --passphrase '' \
        --quick-add-uid "$MFPR" "Injected UID -- DO NOT TRUST <injected@invalid.example>" >/dev/null 2>&1; then
    notok "a UID was added from the operational keyring; the certify-only master is reachable there"
else
    ok "a UID cannot be added from the operational keyring — certification needs the offline master"
fi
if env GNUPGHOME="$OPS" gpg --batch --pinentry-mode loopback --passphrase '' \
        --quick-set-expire "$MFPR" 5y >/dev/null 2>&1; then
    notok "the primary's expiry was changed from the operational keyring"
else
    ok "the primary's expiry cannot be changed from the operational keyring"
fi
env GNUPGHOME="$OPS" gpgconf --kill gpg-agent >/dev/null 2>&1 || true

printf '\n-----------------------------------------------\n'
printf '%d passed, %d failed\n' "$PASS" "$FAIL"
if [ "${#SKIPPED[@]}" -gt 0 ]; then
    printf '%d NOT EXECUTED:\n' "${#SKIPPED[@]}"
    for g in "${SKIPPED[@]}"; do printf '  %s\n' "$g"; done
fi
if [ "$FAIL" -gt 0 ]; then printf '\nFailed:\n'; for g in "${FAILED[@]}"; do printf '  %s\n' "$g"; done; fi
[ "$FAIL" -eq 0 ]
