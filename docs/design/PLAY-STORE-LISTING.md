# Google Play store listing — Pliwee for Android

Listing copy and store graphics for the Android app. Every claim is limited to
what the current release implements, checked against the code on `main` at
`6860541` on 2026-10-06. Declarations (Data safety, FGS, content rating) are in
[`audits/android/ANDROID-PLAY-V1-DECLARATIONS.md`](../audits/android/ANDROID-PLAY-V1-DECLARATIONS.md).

| | |
| --- | --- |
| Package | `io.github.yurisismotto.pliwee` |
| Version | `versionName` 1.1.1, `versionCode` 3 (`android/app/build.gradle.kts`) |
| Android | 10 (API 29) or later; targets API 36 |
| Play history | not yet published to production on Google Play; bundles have been uploaded to Play Console testing tracks |
| Desktop it pairs with | Pliwee 1.1.0 (`pliwee` and `pliwee-gui` packages) |

**Not claimed anywhere below:** automatic phone→computer clipboard, media
control, browser integration, notifications from the computer to the phone,
cloud sync, Windows or macOS. None of them exists in this release. Windows is
planned and macOS is a preview branch on the roadmap; neither is a supported
platform.

---

## Listing text

**App name** (≤ 30): `Pliwee`

**Short description** (≤ 80):

> Send files and clipboard between Android and your Linux PC. No cloud or account.

**Full description** (≤ 4000):

> **One flow. Any device.**
>
> Pliwee connects your Android phone or tablet to your Linux computer
> directly over your home or office Wi-Fi. No account, no cloud, no server in
> between: the two devices talk to each other and to nobody else.
>
> **You need the Pliwee desktop app on your computer.** It is free and open
> source, with packages for Fedora, Ubuntu 24.04 and 26.04, and Debian 13. Get
> it from github.com/yurisismotto/pliwee. Both devices must be on the same
> local network.
>
> **What you can do**
>
> • **Send files both ways.** Share a photo or document to Pliwee from any
>   app, or pick one in Pliwee. Files from your computer land in
>   Download/Pliwee on your phone, and you accept each one first.
>
> • **Send your clipboard to your computer.** Tap Send, use the Quick
>   Settings tile, or share text to Pliwee. Nothing leaves your phone until
>   you ask.
>
> • **Receive your computer's clipboard.** Text copied on your computer can be
>   sent to your phone and copied with one tap, or automatically if you choose.
>   (Sending from the computer is not available on Debian 13.)
>
> • **See your phone's notifications on your computer** — optional and off
>   until you turn it on. You choose which computer and which apps. When your
>   phone is locked, only the app name is sent unless you change it.
>
> • **See your phone's battery on your computer**, and your computer's on your
>   phone if it has one.
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
>   github.com/yurisismotto/pliwee.
>
> **Requirements**
>
> • Android 10 or later.
> • A Linux computer running the Pliwee desktop app (Fedora 41 or later,
>   Ubuntu 24.04 or 26.04, or Debian 13), on the same local network.
> • Pliwee does not work over the internet or mobile data. It is built for
>   devices on the same network.

**What's new — 1.1.1** (≤ 500):

> First release on Google Play. Pair your Android device with the Pliwee
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
| Email | `<OPERATOR: support address to publish>` — none is set in the repository |
| Website | `https://github.com/yurisismotto/pliwee` |
| Privacy policy | `https://yurisismotto.github.io/pliwee/privacy/` — the page rendered from [`policy/PRIVACY-POLICY.md`](../policy/PRIVACY-POLICY.md); the same URL is compiled into the app (`ui/PrivacyPolicy.kt`) |

### Where each claim comes from

| Claim | Source |
| --- | --- |
| Received files in Download/Pliwee, no storage permission | `files/Downloads.kt` (`DIRECTORY_DOWNLOADS/Pliwee` via MediaStore) |
| Every incoming file is accepted first | `files/FileTransferManager.kt`, accept prompt in `ui/TransferViews.kt` |
| Clipboard send is manual only | `clipboard/ClipboardPolicy.kt` (`AUTO_SEND_SUPPORTED = false`), Quick Settings tile `ui/ClipboardTileService.kt` |
| Received clipboard waits for a tap unless automatic is chosen | `ClipboardPolicy.autoReceive = false` by default |
| Clipboard and notifications start off; files and battery allowed at pairing | `capability/SensitiveCapabilities.kt`, copy in `ui/PeerDetailScreen.kt` |
| Locked phone sends only the app name | `notifications/NotificationPolicy.kt` (`LockPolicy.DEFAULT = APP_ONLY`) |
| Computer's battery only if it has one | `desktop/capabilities/battery/src/upower.rs` (receive-only without a battery) |
| TLS 1.3, pinned keys | `net/PinnedTrustManager.kt` |
| Desktop distributions | README "Supported platforms": Fedora 41+ (certified on 44), Ubuntu 24.04 and 26.04 LTS, Debian 13 (manual clipboard sending from the desktop does not work there) |
| No analytics or Play Services | `android/README.md`; no such dependency in `android/app/build.gradle.kts` |

Lengths as measured on 2026-10-06. Re-measure if the text changes.

| Field | Limit | Length |
| --- | --- | --- |
| App name | 30 | 6 |
| Short description | 80 | 80 |
| Full description | 4000 | ≈ 2050 |
| What's new | 500 | 346 |

---

## Store graphics

Requirements as read in the Play help centre (answer/9866151) on 2026-09-24:

| Asset | Requirement | Status |
| --- | --- | --- |
| App icon | 512 × 512, 32-bit PNG with alpha, ≤ 1024 KB | ✅ [`assets/play/play-icon-512.png`](assets/play/play-icon-512.png): the Pliwee mark on Surface `#F7F9FC`, placed exactly as the launcher's adaptive icon shows it (`render.sh` reads the placement and background from the Android resources) |
| Feature graphic | 1024 × 500, JPEG or 24-bit PNG, no alpha; **required to publish** | ✅ [`assets/play/play-feature-graphic-1024x500.png`](assets/play/play-feature-graphic-1024x500.png): the frozen Pliwee lockup [`pliwee-lockup.svg`](assets/pliwee-lockup.svg) (mark, wordmark, tagline) on light surface `#F7F9FC` |
| Phone screenshots | at least 2 overall; 2–8 per device type; JPEG or 24-bit PNG, no alpha; each side 320–3840 px; long side ≤ 2 × short; 4+ at ≥ 1080 px recommended | ✅ four, 1080 × 1920, portrait, 9:16 — [`assets/play/screenshots/phone/`](assets/play/screenshots/phone/) |
| 7-inch / 10-inch tablet screenshots | 4+ recommended, each side 1080–7680 px | ✅ four, 1440 × 2560, portrait, 9:16 — [`assets/play/screenshots/tablet/`](assets/play/screenshots/tablet/) |
| Promo video | optional; YouTube, public or unlisted | not planned; the FGS video is separate and not a promo |

The icon and feature graphic are generated by
[`assets/play/render.sh`](assets/play/render.sh) from the canonical SVGs.
Nothing is redrawn, recoloured or re-set, as BRAND.md § Misuse requires.

### Screenshots — the real app, with demonstration data

| Set | Files | Size | Aspect | Device |
| --- | --- | --- | --- | --- |
| Phone | 4 | 1080 × 1920, portrait | 9:16 | emulator `pliwee-phone`: Pixel 8 profile, panel set to 1080 × 1920, 420 dpi, smallest width ≈ 411 dp |
| Tablet | 4 | 1440 × 2560, portrait | 9:16 | emulator `pliwee-tablet`: Pixel Tablet profile, panel set to 2560 × 1440, 320 dpi, smallest width 720 dp (`sw720dp`, `xlarge`: Android treats it as a tablet) |

Both sets are 24-bit RGB PNGs without an alpha channel, Android 16 (API 36),
debug build of 1.1.1. The phone panel is not the Pixel 8's native 1080 × 2400,
whose 2.22 : 1 would break Play's 2 : 1 limit. The tablet panel is not the Pixel
Tablet's native 2560 × 1600: 9:16 follows Play's large-screen screenshot
guidance as the owner read it on 2026-10-06 — re-check it in the console at
submission. Only the pixel count changes; the app lays itself out for the real
size and density. The tablet is portrait because the shell caps content at
640 dp ([`ui/PliweeShell.kt`](../../android/app/src/main/java/io/github/yurisismotto/pliwee/ui/PliweeShell.kt)
`ContentMaxWidth`): in landscape the screen is a phone-width column between
two empty margins, and that column is what the tablet captures show.

Screens, the same four on both sets:

1. **`01-devices.png`** — Devices: one paired computer, "Linux Desktop",
   Connected, battery 82 %.
2. **`02-device-details.png`** — the computer's detail screen: Clipboard,
   Files, Battery and Notifications.
3. **`03-clipboard.png`** — Send clipboard: the preview of "Hello from Pliwee"
   before sending.
4. **`04-files-or-notifications.png`** — Files: `photo.jpg` (2.4 MB) received,
   `notes.pdf` sending.

**How they are made.** Not a mock-up and not a photo of a design: the real
`PliweeTheme` and `PliweeShell` render a fictional state, and each screen is
reached by tapping, as a person would. The harness is
[`PlayStoreScreenshots`](../../android/app/src/androidTest/java/io/github/yurisismotto/pliwee/PlayStoreScreenshots.kt),
`androidTest` source only: it is not in the release build, nothing in `main`
knows about it, and it skips itself unless the instrumentation argument
`playScreenshots=true` is passed. Each screen is captured with `screencap`.

**What is in them.** Demonstration data only. No personal data, no real
computer, no IP address, no QR code, no real notification, no fingerprint on
screen (the state's key is a made-up constant). The status bar is Android's
demo mode: 10:00, Wi-Fi, full battery. No device frame, no overlaid slogan,
no edit after capture beyond dropping the fully opaque alpha channel, which
the script checks leaves every pixel unchanged.

**How to regenerate and check them:**

```bash
cd android && ./gradlew :app:assembleDebug :app:assembleDebugAndroidTest && cd ..
android/tools/capture-play-screenshots.sh phone
android/tools/capture-play-screenshots.sh tablet
android/tools/capture-play-screenshots.sh verify   # no emulator: size, 9:16, no alpha, exactly 4 per set
```

[`capture-play-screenshots.sh`](../../android/tools/capture-play-screenshots.sh)
carries the one-time emulator setup. It boots its own emulator and refuses a
serial that is already attached, measures the panel size and smallest width
before capturing (≥ 600 dp for the tablet, < 600 dp for the phone), and
fails unless exactly four PNGs of the expected size come back.

The six tablet screenshots captured on 2026-09-24 from the pre-rename release
build on the certification tablet are no longer store assets. They are kept
in git history (`ba408c6`), and the smoke report that produced them,
[`reports/android/ANDROID-PLAY-V1-RELEASE-SMOKE.md`](../reports/android/ANDROID-PLAY-V1-RELEASE-SMOKE.md)
§4, still describes them as they were.
