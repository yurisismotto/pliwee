# ADR-0019 — Android app signing: maintainer-owned key, Play App Signing, separate upload key

Status: Accepted (2026-09-24). Operator decision recorded at PLAY4 of the
Android / Google Play v1 wave; the options as they were put are in
[`audits/android/ANDROID-PLAY-V1-READINESS-AUDIT.md` §6](../audits/android/ANDROID-PLAY-V1-READINESS-AUDIT.md#6-play4--app-signing-model-operator-decision).

> **Superseding note — 2026-09-24, Pliwee rebrand Wave 6 (branch `feature/pliwee-rebrand-wave6`), [ADR-0020](ADR-0020-rename-to-pliwee.md) §D1 and §D3.** The model, algorithm, validity, two-key split, custody and backup rules below still hold. The *identity* they were applied to is retired unused: the app is now `io.github.yurisismotto.pliwee`, signed by a new Pliwee identity (`pliwee-app-signing` / `pliwee-upload`, `CN=Pliwee, …`), provisioned by the same procedure. The OmniBridge certificates, record and backups below are preserved and never overwritten; the certificates moved, byte-identical, to `android/signing/certs/legacy-omnibridge/`. The names, paths and environment variables in this ADR (`omnibridge-*`, `OMNIBRIDGE_UPLOAD_KEYSTORE`) describe the OmniBridge identity as it was decided. See [Addendum — Pliwee signing identity](#addendum--pliwee-signing-identity-adr-0020-d3) at the end.

## Context

Every installed copy of an Android app is bound for life to the certificate
that signed it: Android installs an update only if it is signed by the same key
(or a rotation the platform recognises). Google Play offers two ways to hold
that key under Play App Signing — Google generates it and nobody else ever
has it, or the developer generates it and gives Play a copy. Either way uploads
are signed with a separate, resettable upload key.

OmniBridge ships from GitHub today, avoids Google Play Services (ADR-0009
rejected cloud push on principle; the QR scanner is zxing rather than ML Kit),
and its users are the ones likely to move between GitHub, F-Droid
and Play. A key only Google holds would make each of those channels a
different app.

## Decision

**The maintainer generates and keeps the long-term app signing key, and
supplies it to Play App Signing. A separate upload key signs Play uploads.**

| | App signing key | Upload key |
| --- | --- | --- |
| Purpose | the identity every installed copy is bound to; Play re-signs with it | proves an upload came from the maintainer |
| Algorithm | RSA-4096, SHA256withRSA, PKCS#12 | RSA-4096, SHA256withRSA, PKCS#12 |
| Validity | 30 years (Play requires beyond 2033-10-22) | 30 years |
| Subject (public in every APK) | `CN=OmniBridge, OU=Android App Signing` | `CN=OmniBridge, OU=Android Upload` |
| Alias | `omnibridge-app-signing` | `omnibridge-upload` |
| Lives | **offline only**: encrypted backups on two media, two places | the maintainer's workstation, outside the repository, mode 0600 |
| Also held by | Google Play App Signing (after the PEPK step) | nobody — Play holds only its certificate |
| If lost | restore from either medium; if both are gone, Play keeps signing but off-Play distribution under this identity ends | restore from backup, or reset through Play Console |
| If leaked | an attacker can sign sideload "updates" that install over genuine copies — treat as an incident; Play-side key upgrade exists but older Android versions keep trusting the old key | request an upload key reset; the leaked key cannot publish on Play alone |

The two keys are distinct keys, not two aliases for one key. The provisioning
script checks that their certificates and moduli differ.

### What the repository may and may not contain

* **May:** how signing is supplied (Gradle reading an external keystore path
  and environment-supplied passwords — PLAY6), public certificates, and public
  SHA-256 / SHA-1 certificate fingerprints.
* **May not:** any keystore, any private key in any encoding, any password,
  any properties file naming a password, or the encrypted backup. `.gitignore`
  covers `*.jks *.keystore *.p12 *.pfx key.properties keystore.properties
  signing.properties`; ignoring is a second line of defence, not the first.
* **GitHub Actions** receives no private signing material. Changing that is a
  separate decision needing explicit operator approval and its own ADR.

## Custody — the provisioning procedure

One resumable script, run by the operator in their own terminal:
[`android/signing/provision-signing-keys.sh`](../../android/signing/provision-signing-keys.sh).
It follows the principle the OpenPGP release key was provisioned under
([release signing foundation §8.5](../audits/release/RELEASE-SIGNING-FOUNDATION-V1.md)):
*an unverified backup is a belief, not a backup.*

| Stage | What happens | Where secrets are |
| --- | --- | --- |
| 1 generate | both keys and three 192-bit random passwords (app keystore, upload keystore, backup) | `$XDG_RUNTIME_DIR` — tmpfs, checked, mode 0700 |
| 2 show | the three passwords printed **once**; the operator saves them in a password manager and types `SAVED`; the screen and scrollback are cleared | password manager |
| 3 backup | one bundle (both keystores + public certificates + checksums) encrypted with gpg AES-256 (S2K SHA-512, 65 011 712 iterations, a throwaway `GNUPGHOME`, no passphrase cache) written to each of two separate mounted media; the script then stops and requires both to be ejected and re-inserted | media A and B, encrypted |
| 4 verify | the operator pastes all three passwords **from the password manager**; each must equal what was generated; each bundle is read back from the **re-mounted** media (mount id must have changed), checksummed, decrypted, compared byte for byte, and each private key must sign a CSR whose public key matches its certificate | tmpfs |
| 5 install | the **upload** keystore only, to `~/.local/share/omnibridge-android-signing/` (not the desktop daemon's `~/.local/share/omnibridge`, which a pairing reset may remove) | workstation |
| 6 destroy | the tmpfs directory is shredded; a **public** record (fingerprints, bundle hashes, dates) is written to `~/.local/state/omnibridge-android-signing/PROVISIONED` | — |

It refuses to run once `PROVISIONED` exists: a second app signing key would be
a second app. It refuses media that are not mount points, that share a device
with each other or with `$HOME`, or that lie inside the repository.

The procedure is rehearsed by
[`android/signing/tests/provision-selftest.sh`](../../android/signing/tests/provision-selftest.sh)
with throwaway keys under a scratch root — the full run plus each refusal
(no media, same medium twice, media in the repository, unconfirmed passwords,
no re-mount, a mistyped password, a corrupted bundle, a second provisioning),
an independent restore by the steps printed on the media, and checks that no
password or plaintext private key reaches the media, the record or the
repository. Android CI runs it.

### Supplying the key to Play

Not part of provisioning. When the Play app exists, Play Console offers its
per-app encryption public key; the app signing keystore is restored from one
medium into tmpfs, exported with Google's PEPK tool under that key, uploaded,
and the tmpfs copy destroyed. The upload certificate (public) is registered at
the same time. That step is an operator action at PLAY18.

### Recovery

| Event | Response |
| --- | --- |
| workstation lost or wiped | restore the upload keystore from either medium |
| upload key suspected leaked | Play Console › Protected with Play › Play Store protection › Manage Play app signing › Request upload key reset (path as the help centre gave it on 2026-09-24), with a newly generated upload certificate; record the new public fingerprint with a dated note |
| one medium lost or unreadable | the other restores; write a fresh copy to new media and restore-verify it |
| both media lost | Play keeps signing with its copy; GitHub / F-Droid releases under this identity can no longer be produced. Record it; do not generate a replacement app signing key silently |
| app signing key suspected leaked | incident: announce, stop off-Play releases, use Play's app signing key upgrade, and record which Android versions still trust the old key |
| a password manager entry lost | the backup password loses both bundles; a keystore password loses that key. There is no other copy — this is why stage 4 makes the operator paste each one back |

## Consequences

* A GitHub-built APK signed with the app signing key updates a Play-installed
  copy and the other way round. F-Droid can ship maintainer-signed APKs if the
  build is made reproducible; otherwise F-Droid signs with its own key, as it
  would under either model.
* Producing a non-Play release is a deliberate local act with a medium mounted,
  like signing a Linux release — never a CI job.
* The maintainer carries a key that must outlive the project. The cost of that
  is two drives, three password-manager entries and a procedure that refuses
  to call a backup good until it has been restored.
* The help centre does not state whether a developer-supplied key receives the
  hybrid (RSA-4096 + ML-DSA-65) signing Google-generated keys get by default;
  what Play Console shows at PEPK time is recorded when it happens.

## Provisioned identity (public)

Provisioned by the operator on 2026-09-24 with the procedure above; both
offline backups restore-verified at 2026-09-24T05:05:45Z. The certificates
are committed as public files in
[`android/signing/certs/`](../../android/signing/certs/).

| Key | Certificate SHA-256 | SHA-1 | Valid |
| --- | --- | --- | --- |
| app signing (`omnibridge-app-signing`) | `AB:B6:2F:53:CB:63:32:6D:AE:3D:02:A2:5C:76:CA:45:CA:2B:95:B5:63:8A:95:5B:BB:7B:45:20:B0:23:AC:49` | `73:EB:FB:A7:90:50:7F:0D:E5:99:2A:AE:8E:69:EF:CE:91:CD:D6:F0` | 2026-09-24 → 2056-09-23 |
| upload (`omnibridge-upload`) | `75:FC:88:B5:20:72:47:EF:21:50:6B:D5:F6:EB:87:A8:15:AE:C0:13:87:41:83:30:3E:42:6A:92:7E:2F:BA:47` | `9D:65:32:06:A1:0E:77:C6:09:23:CA:32:20:AB:91:B1:BA:9B:2C:C0` | 2026-09-24 → 2056-09-23 |

After the PEPK step, Play Console's *App signing* page must show the app
signing SHA-256 above and the upload SHA-256 above. Any other value there
means a different key reached Play, and nothing is published until it is
explained.

## Building a release

`android/signing/build-release-bundle.sh` prompts for the upload keystore
password (hidden), passes it to one `--no-daemon` Gradle run through the
environment, refuses a dirty working tree, checks the password did not reach
the bundle, and runs `android/signing/verify-release-bundle.sh`, which fails
unless the bundle has exactly one signer whose certificate is the committed
upload certificate, is not debug-signed, verifies under `jarsigner`, and
carries the expected package, minSdk, targetSdk ≥ 36, no `debuggable`,
exactly the expected permissions, no key material, and an R8 mapping.

In `app/build.gradle.kts`, any task producing a release APK or bundle fails
when `OMNIBRIDGE_UPLOAD_KEYSTORE` / `OMNIBRIDGE_UPLOAD_KEYSTORE_PASSWORD` are
absent or the keystore lies inside the repository. Android CI asserts that
refusal.

## Addendum — Pliwee signing identity (ADR-0020 §D3)

*Added 2026-09-24, Pliwee rebrand Wave 6. Everything above is left as decided.*

The OmniBridge identity above is **retired unused**: it never signed a
distributed artifact, and the OmniBridge Play app was never created. It is not
deleted: its certificates are kept byte-identical in
[`android/signing/certs/legacy-omnibridge/`](../../android/signing/certs/legacy-omnibridge/)
(SHA-256 of the files: `f76294049cf92ff42bd4c6d5005443579fdb605753ff693f74f032d513f1c3f4`
app signing, `f7b9213007c709a5c4cdaeb7e2b107c490bc1e695410abff9333a6518d80f06e`
upload), and its record, installed upload keystore, offline backups and
password-manager entries stay where they are.

The Pliwee identity follows every rule of this ADR; only the subject, the
aliases and the paths change:

| | Pliwee |
| --- | --- |
| Package | `io.github.yurisismotto.pliwee` |
| App signing | alias `pliwee-app-signing`, `CN=Pliwee, OU=Android App Signing`, RSA-4096, 30 years |
| Upload | alias `pliwee-upload`, `CN=Pliwee, OU=Android Upload`, RSA-4096, 30 years |
| Custody | `~/.local/share/pliwee-android-signing/` (upload keystore only), `~/.local/state/pliwee-android-signing/PROVISIONED` |
| Offline backup | `pliwee-android-signing/pliwee-android-signing-v1.tar.gpg` on each of two media |
| Build environment | `PLIWEE_UPLOAD_KEYSTORE`, `PLIWEE_UPLOAD_KEYSTORE_PASSWORD`; `-Ppliwee.release.unsigned=true` |
| Public record | `android/signing/certs/{app-signing,upload}-certificate.pem` |

`provision-signing-keys.sh` now takes this identity as data. Its refusal is
kept and read as *one signing identity per `applicationId`*: an existing
OmniBridge record does not stop Pliwee provisioning and is left byte-identical;
an existing Pliwee record refuses a second run. Both cases are in
`android/signing/tests/provision-selftest.sh`.
`verify-release-bundle.sh` expects exactly one signer whose certificate is the
**Pliwee** upload certificate, the package `io.github.yurisismotto.pliwee`, and
refuses to verify against either retired certificate.

### Pliwee certificates

**Not yet provisioned (2026-09-24).** No key was generated by Wave 6: the
plan forbids generating keys in CI or in an assistant session, and the
procedure prints passwords that must reach only the operator. The operator
runs `android/signing/provision-signing-keys.sh --media-a … --media-b …` in
their own terminal with two removable media, commits the two public
certificates, and records here:

| Key | Certificate SHA-256 | SHA-1 | Valid |
| --- | --- | --- | --- |
| app signing (`pliwee-app-signing`) | `81:86:C0:05:D0:10:CF:32:E0:1A:4D:CC:75:BB:6D:1E:42:E2:21:77:40:32:95:9C:B7:00:BF:E8:B4:68:64:44` | `9C:A0:E7:A4:23:70:A5:85:0B:ED:8D:05:17:27:DB:82:46:EB:2B:1B` | 2026-09-25 21:39:43 → 2056-09-24 21:39:43 UTC |
| upload (`pliwee-upload`) | `67:DE:80:2F:32:04:2A:F7:F1:F6:DB:67:48:E9:85:17:70:05:26:D4:C8:06:98:35:6E:F5:D4:85:D0:B1:73:73` | `F0:81:F5:88:4F:80:EF:89:FA:9A:FD:DC:91:9B:52:76:90:55:D4:05` | 2026-09-25 21:39:44 → 2056-09-24 21:39:44 UTC |

> **2026-10-03 — provisioned.** The operator ran `provision-signing-keys.sh`
> at 2026-09-25T21:39:44Z; both offline backups were restore-verified at
> 2026-09-25T21:43:33Z (`~/.local/state/pliwee-android-signing/PROVISIONED`).
> The two public certificates are committed in `android/signing/certs/`,
> byte-identical to the provisioned ones (file SHA-256
> `05e404db906246e47d650e37459c6bcb6dcb85e60ba1c826f564bfcc3a466b84` app signing,
> `3a897e583a86400563aa3d1d5bcb5cf4bc6424e3340503a23614d95d2e0769c8` upload).
> The table above was filled in from them; the "not yet provisioned" heading
> it sits under describes 2026-09-24.

Rollback: until the PEPK step of the Play release wave (ADR-0020, W10) the
Pliwee identity is enrolled nowhere, so discarding it costs only the media;
after PEPK it is permanent, as above.
