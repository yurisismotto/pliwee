# Android signing — public certificates

Public certificates only. No private key, keystore, password or backup is
ever committed here (ADR-0019).

| Path | Identity | Status |
| --- | --- | --- |
| `upload-certificate.pem`, `app-signing-certificate.pem` | **Pliwee** — `CN=Pliwee, OU=Android Upload` / `CN=Pliwee, OU=Android App Signing`, aliases `pliwee-upload` / `pliwee-app-signing` | **provisioned 2026-09-25**, both media restore-verified; committed 2026-10-03. Fingerprints in the ADR-0020 addendum of [ADR-0019](../../../docs/adr/ADR-0019-android-app-signing.md). `../verify-release-bundle.sh` verifies against `upload-certificate.pem`. |
| `legacy-omnibridge/*.pem` | **OmniBridge** (ADR-0019), retired unused by ADR-0020 §D3 | kept **byte-identical** (moved here with `git mv` in Pliwee Wave 6). They never sign a Pliwee artifact; `verify-release-bundle.sh` refuses to verify against them. |

Retired OmniBridge fingerprints (SHA-256), for comparison:

* app signing: `AB:B6:2F:53:CB:63:32:6D:AE:3D:02:A2:5C:76:CA:45:CA:2B:95:B5:63:8A:95:5B:BB:7B:45:20:B0:23:AC:49`
* upload: `75:FC:88:B5:20:72:47:EF:21:50:6B:D5:F6:EB:87:A8:15:AE:C0:13:87:41:83:30:3E:42:6A:92:7E:2F:BA:47`
