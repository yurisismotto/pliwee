# Google Play store listing — OmniBridge v1 (PLAY11 · PLAY12)

Listing copy and store graphics for the Android app. Every claim is limited to
what v1 implements, checked against the code on `feature/android-google-play-v1`.
Declarations (Data safety, FGS, content rating) are in
[`audits/android/ANDROID-PLAY-V1-DECLARATIONS.md`](../audits/android/ANDROID-PLAY-V1-DECLARATIONS.md).

**Not claimed anywhere below:** automatic phone→computer clipboard, media
control, browser integration, cloud sync, Windows or macOS. None of them
exists in v1.

---

## Listing text

**App name** (≤ 30): `OmniBridge`

**Short description** (≤ 80):

> Send files and clipboard between Android and your Linux PC. No cloud or account.

**Full description** (≤ 4000):

> **One bridge. Any device.**
>
> OmniBridge connects your Android phone or tablet to your Linux computer
> directly over your home or office Wi-Fi. No account, no cloud, no server in
> between: the two devices talk to each other and to nobody else.
>
> **You need the OmniBridge desktop app on your computer.** It is free and open
> source, with packages for Fedora, Ubuntu 24.04 and 26.04, and Debian 13. Get
> it from github.com/yurisismotto/OmniBridge. Both devices must be on the same
> local network.
>
> **What you can do**
>
> • **Send files both ways.** Share a photo or document to OmniBridge from any
>   app, or pick one in OmniBridge. Files from your computer land in
>   Download/OmniBridge on your phone, and you accept each one first.
>
> • **Send your clipboard to your computer.** Tap Send, use the Quick
>   Settings tile, or share text to OmniBridge. Nothing leaves your phone until
>   you ask.
>
> • **Receive your computer's clipboard.** Text copied on your computer can be
>   sent to your phone and copied with one tap, or automatically if you choose.
>
> • **See your phone's notifications on your computer** — optional and off
>   until you turn it on. You choose which computer and which apps. When your
>   phone is locked, only the app name is sent unless you change it.
>
> • **See your phone's battery on your computer**, and your computer's on your
>   phone.
>
> **Private by design**
>
> • Pair by scanning a QR code on your computer's screen. Each device then
>   accepts only the other.
> • Everything travels over an encrypted connection (TLS 1.3) directly between
>   your two devices.
> • No ads, no analytics, no tracking, no account.
> • Every feature has its own switch, per computer. Clipboard and notifications
>   start off.
> • Open source: read the code and the privacy policy at
>   github.com/yurisismotto/OmniBridge.
>
> **Requirements**
>
> • Android 10 or later.
> • A Linux computer running the OmniBridge desktop app, on the same local
>   network.
> • OmniBridge does not work over the internet or mobile data. It is built for
>   devices on the same network.

**What's new — 1.0.0** (≤ 500):

> First release on Google Play. Pair your Android device with the OmniBridge
> desktop app on Linux to send files both ways, send your clipboard to your
> computer, receive your computer's clipboard, and optionally see your
> phone's notifications on your computer. All over a direct, encrypted
> connection on your local network, with no account and no cloud.

**Category:** Tools. (Productivity also fits. Tools matches how people search
for a device-to-device bridge, and it is not a borderline call.)

**Tags:** choose from Play's list at submission time. Do not stuff them.

**Contact details** (Play requires an email address; it appears on the listing):

| Field | Value |
| --- | --- |
| Email | `<OPERATOR: support address to publish>` |
| Website | `https://github.com/yurisismotto/OmniBridge` |
| Privacy policy | `https://github.com/yurisismotto/OmniBridge/blob/main/docs/policy/PRIVACY-POLICY.md` |

Lengths as measured on 2026-09-24. Re-measure if the text changes.

| Field | Limit | Length |
| --- | --- | --- |
| App name | 30 | 10 |
| Short description | 80 | 80 |
| Full description | 4000 | ≈ 2030 |
| What's new | 500 | 350 |

---

## Store graphics (PLAY12)

Current requirements (help centre answer/9866151, read 2026-09-24):

| Asset | Requirement | Status |
| --- | --- | --- |
| App icon | 512 × 512, 32-bit PNG with alpha, ≤ 1024 KB | ✅ [`assets/play/play-icon-512.png`](assets/play/play-icon-512.png): the Pliwee mark on Surface `#F7F9FC`, placed exactly as the launcher's adaptive icon shows it (the OmniBridge icon on Dark `#0B1020` until 2026-10-03; `render.sh` reads the placement and background from the Android resources) |
| Feature graphic | 1024 × 500, JPEG or 24-bit PNG, no alpha; **required to publish** | ✅ [`assets/play/play-feature-graphic-1024x500.png`](assets/play/play-feature-graphic-1024x500.png): the frozen Pliwee lockup [`pliwee-lockup.svg`](assets/pliwee-lockup.svg) (mark, wordmark, tagline) on light surface `#F7F9FC` (the OmniBridge lockup until 2026-10-03) |
| Phone screenshots | at least 2 overall; 2–8 per device type; each side 320–3840 px; long side ≤ 2 × short; 4+ at ≥ 1080 px recommended | **pending** — from the real app |
| 7-inch / 10-inch tablet screenshots | 4+ recommended, each side 1080–7680 px | ✅ six, 1800 × 2724, from the release build on SM-X620 — [`assets/play/screenshots/tablet/`](assets/play/screenshots/tablet/) (status bar and taskbar cropped; see the smoke report §4) |
| Promo video | optional; YouTube, public or unlisted | not planned; the FGS video is separate and not a promo |

Both graphics are generated by [`assets/play/render.sh`](assets/play/render.sh)
from the canonical SVGs. Nothing is redrawn, recoloured or re-set, as
BRAND.md § Misuse requires.

### Screenshots — from the real app only

No screenshot is mocked up. They are captured from the release build on the
certification device with `adb exec-out screencap -p` during the PLAY17 smoke,
with a real paired computer. Screens:

1. **Devices** — one paired computer, connected.
2. **Computer detail** — the per-computer permission switches.
3. **Send clipboard** — the preview before sending.
4. **Incoming file** — the accept prompt, then the finished transfer.
5. **Notifications** — the per-computer screen with a few apps chosen.
6. **The disclosure** — "Share notifications with *computer*?"
7. **Settings** — this device's fingerprint and the privacy section.
8. **Pairing** — the scanner (captured with a sample QR on screen).

Before capture: set a neutral device name on the computer, and make sure no
personal notification, file name or clipboard text is visible.

**Phone-size screenshots** need a phone. SM-X620 is a tablet. Play needs at
least two screenshots in total to publish. Whether a listing with only tablet
screenshots is acceptable for phone users is checked in the console at
submission, not assumed here.
