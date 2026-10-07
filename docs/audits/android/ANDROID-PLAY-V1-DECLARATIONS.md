# Android / Google Play v1 — Play Console declarations (PLAY8 → PLAY15)

| | |
| --- | --- |
| Date | 2026-09-24 |
| Branch | `feature/android-google-play-v1` |
| Code basis | commit `1e1ebc1` (F1–F5 fixed) |
| Companion | [readiness audit](ANDROID-PLAY-V1-READINESS-AUDIT.md) · [privacy policy](../../policy/PRIVACY-POLICY.md) · [ADR-0019](../../adr/ADR-0019-android-app-signing.md) · [store listing](../../design/PLAY-STORE-LISTING.md) |

These are the answers to give in Play Console, and the evidence behind each.
Where an answer rests on how Play reads its own policy rather than on the code,
it is marked **⚖ interpretation**. The operator makes those calls; the code
evidence is here so the call is informed.

Policy sources were read on 2026-09-24:
User Data (answer/10144311), Data safety (answer/10787469), FGS
(answer/13392821, answer/13315670), app testing (answer/14151465),
developer verification (developer.android.com/developer-verification).

---

## PLAY8 — prominent disclosure and consent

The rule: an in-app disclosure, in the normal flow, that says what data is
accessed, how it is used and where it goes. It must come **before** access and
end in an **affirmative action**. Going back, leaving the screen or a
disclosure that dismisses itself does not count as consent.

### Notification access and notification mirroring

The listener can run while the activity is in the background, and
notifications carry other people's messages, so this is the one flow that most
needs the disclosure.

| Requirement | Evidence (after F3) | Verdict |
| --- | --- | --- |
| Disclosure before access | Turning on *Share notifications with this computer* opens `NotificationDisclosureDialog` (`ui/NotificationSettingsScreen.kt`) **before** the grant is written. The *Open Android settings* button, the only route to `ACTION_NOTIFICATION_LISTENER_DETAIL_SETTINGS`, is rendered only once the grant exists | ✅ |
| Purpose | "Share notifications with *computer*?" — shown on the computer the person named | ✅ |
| What data | `notif_disclosure_what` lists the app's name and package name, title and text, time, category, importance, progress and ongoing/dismissible, and names what is never sent. This matches `NotificationWire` field for field | ✅ |
| Where it goes | `notif_disclosure_where`: only that computer, local network, encrypted and pinned; never the developers or a cloud; nothing stored | ✅ |
| When active | `notif_disclosure_when`: only while connected to that computer, **including in the background**; visible as an active app and an ongoing notification; lock-screen default | ✅ |
| Separate affirmative action | Only **Allow** grants. **Not now**, back and tapping outside write nothing. There is then a second, separate Android system grant, and then an app picker with nothing pre-selected | ✅ |
| Revocability | `notif_disclosure_next`, plus the one-tap switch-off (no confirmation), Android settings, and revoking the computer | ✅ |
| Deny by default | grant off; `allowedApps` empty; ongoing off; work profile off; locked → app name only; dismiss sync off (`NotificationPolicy.kt`) — **unchanged** | ✅ |
| Listener bound only while needed | `default_autobind=false`; `requestRebind` only while a granted peer is connected (`NotificationSource.updateBinding`) — **unchanged** | ✅ |

Instrumented tests (`NotificationConsentUiTest`) cover this: the disclosure
appears before any grant, declining writes nothing, switching off needs no
confirmation, and the settings button is absent until the grant exists. They
compile, but have **not been run on hardware yet** (PLAY17).

This closes **OQ-09**, which ADR-0015 and the threat model left open for any
Play submission. The ADR itself is not edited; this document is the record.

### The other sensitive surfaces

| Surface | Access | Disclosure / consent | Verdict |
| --- | --- | --- | --- |
| Camera | QR only, frames decoded in memory | Android runtime prompt when the person taps **Pair**; purpose is the action they just took | reasonably expected — no extra disclosure needed |
| Clipboard, phone → computer | read only on the person's Send / tile / share action; never monitored | the action is the consent; sensitive clips get a second confirmation | ✅ |
| Clipboard, computer → phone | held in memory ≤ 5 min, applied on tap unless auto-receive is on for that computer | per-computer grant, **off** by default | ✅ |
| Files | only files the person picks; each incoming file is approved | per-transfer approval | ✅ |
| Device information | random device id, model name, key fingerprint, battery | battery now honours its per-computer switch (F1); card text no longer claims nothing is granted automatically | ✅ |
| Notifications permission | asked when the person pairs or connects (F2) | Android runtime prompt, in context | ✅ |

---

## PLAY9 — Data safety

### What leaves the device, audited

| Flow | Direction | Leaves the device? | To whom | Retained by the recipient | User control |
| --- | --- | --- | --- | --- | --- |
| clipboard text (≤ 32 KiB) + hash + sensitive hint | phone → computer | yes, on the person's action only | the paired computer | desktop applies it to its clipboard | per-computer grant, **off** by default |
| clipboard text | computer → phone | n/a (arrives) | — | memory ≤ 5 min, then the system clipboard on tap | per-computer grant |
| file bytes + name, size, MIME, hash | both | yes, on the person's pick; incoming needs approval | the paired computer | saved where the person's desktop saves downloads | per-computer grant, on at pairing |
| notification app name + package, title, text, timing, category, importance, progress, flags | phone → computer | yes, only while connected to a computer holding the grant, from chosen apps | the paired computer | shown, not stored (`notif_privacy_body`) | three separate gates, all off by default |
| battery %, charging state | phone → computer | yes, on connect | the paired computer | shown | per-computer switch (F1) |
| random device id, model name, key fingerprint, capability list | phone → computer | yes, on connect | the paired computer | the desktop's trust store | pairing is the person's act; revocable |

**No developer server exists.** The app makes no HTTP request, and it has no
SDK that sends anything: no analytics, ads, crash reporting or Play services.
The dependency list is in the readiness audit §2.3. Nothing is shared with a
third party. The recipient is always a computer the person paired themselves,
using a QR code on that computer's screen.

### How Play's definitions apply

* Play: *"collect" means transmitting data from your app off a user's device.*
  Everything above is transmitted off the device, so the absence of a cloud is
  **not** by itself a reason to answer "no data collected".
* Play excludes data that is *"sent off device, but … unreadable by you or
  anyone other than the sender and recipient as a result of end-to-end
  encryption"*. The OmniBridge link is TLS 1.3 with mutual authentication and
  SPKI pinning **directly between the two endpoints**. There is no relay and no
  server, and the recipient is the person's own computer. So nobody but the
  sender and the recipient can read it, and that includes the developer.

### Recommended answers — ⚖ interpretation

**Recommended: "Does your app collect or share any of the required user data
types?" → No**, on the end-to-end-encryption exemption above. It is not based
on the absence of a cloud.

State the basis in the review notes, so a reviewer does not mistake it for
"we have no server":

> OmniBridge transfers data only between the user's own devices that the user
> pairs by scanning a QR code: the phone and the user's computer running the
> OmniBridge desktop software. Transfers are end-to-end encrypted (TLS 1.3,
> mutual authentication, public-key pinning) directly between those two
> devices, with no server in between. The developer operates no server and
> cannot read any of this data. The app contains no analytics, advertising
> or crash-reporting SDK.

**Fallback, if the operator prefers to declare, or Play disputes the
exemption.** Collected, **not shared**:

| Play data type | Why | Optional? | Processed ephemerally? | Purpose |
| --- | --- | --- | --- | --- |
| Messages › Other in-app messages | notification title and text (they may quote SMS, email or chat previews) | optional | yes — shown, not stored | App functionality |
| App activity › Installed apps | package name and label of notifying apps the person chose | optional | yes | App functionality |
| App activity › Other user-generated content | clipboard text | optional | yes | App functionality |
| Files and docs | files the person sends | optional | no — saved by the recipient at the person's request | App functionality |
| Photos and videos | only if the person sends a photo or video as a file | optional | no — as above | App functionality |
| Device or other IDs | random app-generated device id, key fingerprint | required | no — the paired computer keeps it until unpaired | App functionality |
| App info and performance › Other | battery level and charging state | optional | yes | App functionality |

Security practices for the fallback: **encrypted in transit: Yes**.
**Users can request deletion: No.** The developer holds nothing to delete, and
the person deletes it on their own devices by revoking the computer or
uninstalling (⚖ interpretation). **Independent security review: No.**

---

## PLAY10 — foreground service (`connectedDevice`)

Manifest: `ConnectionService`, `foregroundServiceType="connectedDevice"`;
permissions `FOREGROUND_SERVICE`, `FOREGROUND_SERVICE_CONNECTED_DEVICE`, and
the platform prerequisite `CHANGE_WIFI_MULTICAST_STATE` /
`CHANGE_NETWORK_STATE`, both genuinely used (mDNS `MulticastLock`).
`startForeground(…, FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE)` on API 34+.

Policy fit: Play's connectedDevice use case is *"interactions with external
devices including data transfer that require a … network connection"*. The
service exists only while a connection to the person's own paired computer is
up or being set up. It is started by the person, shown to them, stopped by
them, and never started at boot (no `BOOT_COMPLETED` receiver,
`START_NOT_STICKY`).

### Proposed declaration answers

| Question | Answer |
| --- | --- |
| Use case | Connected device — continuous data transfer to the user's paired external device (their computer) over the local network |
| Functionality description | OmniBridge keeps an encrypted, mutually authenticated connection open between the user's phone and a computer the user paired by scanning a QR code. Over it, the user sends and receives files, sends clipboard text, and — only if they turn it on — mirrors chosen apps' notifications to that computer. The connection has to stay up while the screen is off or the user is in another app, so that transfers finish and incoming files and clipboard text arrive. |
| Why it must start promptly | It starts when the user taps Connect or Pair, or shares something to OmniBridge. The user is waiting for the computer to connect, and a queued file or clipboard send completes over that connection. |
| Impact if deferred | The user's computer shows the phone as unreachable. A file or clipboard text the user just sent is not delivered. Files and clipboard text sent from the computer are refused because the phone is not there to receive them. |
| Impact if interrupted | A file transfer in progress fails and must be restarted. Incoming clipboard text and file offers from the computer are lost. Notification mirroring stops without warning. |
| How the user starts it | Tapping **Connect** on a paired computer, completing **Pair** after scanning the computer's QR code, or sharing a file or text to OmniBridge from another app. Never automatically at boot. |
| How the user knows it is active | An ongoing notification, "Connected to *computer*" / "Connecting…", plus the connection state on the Devices screen and Android's active-apps list |
| How the user stops it | **Disconnect** in the app, revoking or removing the computer, or Android's active-apps stop control. It also stops by itself when the connection gives up. |

### Video — shot list for the operator (not fabricated here)

Record the screen of the SM-X620 (Android's built-in screen recorder), about
60–90 seconds, no editing needed:

1. The OmniBridge desktop app on the computer showing its pairing QR code
   (film the monitor, or cut to a desktop screen recording).
2. On the phone: open OmniBridge → **Pair** → scan → "Paired with *computer*".
3. Pull down the shade: the **"Connected to *computer*"** ongoing notification.
4. Press Home, turn the screen off for a few seconds, turn it on. The
   notification is still there.
5. From the computer, send a file to the phone. The phone asks to accept it,
   and it arrives while OmniBridge was in the background.
6. From the phone's Gallery or Files app, share a file to OmniBridge. It
   arrives on the computer.
7. Open OmniBridge → **Disconnect**. The ongoing notification disappears.

Upload it to YouTube as **Unlisted**. Play needs the URL. **STOP point:** the
declaration cannot be submitted until that URL exists.

---

## PLAY13 — App content checklist

> **Superseding note — 2026-10-06, branch `fix/privacy-policy-pliwee`.** The privacy-policy URL to declare is now **`https://yurisismotto.github.io/pliwee/privacy/`**, the GitHub Pages rendering of [`docs/policy/PRIVACY-POLICY.md`](../../policy/PRIVACY-POLICY.md), which was rewritten as the **Pliwee Privacy Policy** for `io.github.yurisismotto.pliwee`. The same URL is compiled into the app from versionCode 3 (`ui/PrivacyPolicy.kt`). It resolves only once GitHub Pages is enabled for `/docs` on `main`. The OmniBridge URL in the row below was compiled into the versionCode 2 bundle and is retired. The text below is left as it was written.

| Section | Answer | Basis |
| --- | --- | --- |
| Privacy policy | `https://github.com/yurisismotto/OmniBridge/blob/main/docs/policy/PRIVACY-POLICY.md` | **Resolves only after this branch is merged to `main`.** Merge before the first review |
| Ads | **No**, the app contains no ads | no ad SDK |
| App access | ⚖ **All or some functionality is restricted**, with instructions. No login exists, but nothing works without a paired desktop peer. Instructions: install the OmniBridge desktop release from the GitHub Releases page on a Linux computer on the same Wi-Fi, run it, and scan its pairing QR code in the app. Also point to the FGS video | a reviewer without a desktop sees only the pairing screen |
| Target audience | **Operator decision** — see below | |
| Content rating (IARC) | Category: **Utility, Productivity, Communication, or Other**. Violence, sexuality, language, controlled substances, gambling: **No**. "Users interact or exchange content": ⚖ **No**, because exchange is only between the user's own devices and never with other users. Shares location: **No**. Digital purchases: **No**. Unrestricted web access: **No** | |
| Data safety | PLAY9 above | |
| Foreground service | PLAY10 above; video URL required | |
| Advertising ID | **No**: the app does not use it and does not declare `AD_ID` | merged manifest |
| Sensitive permission forms | none expected. No SMS, call log, all-files, `QUERY_ALL_PACKAGES`, background location, accessibility, exact alarm or photo/video permission is requested. If the console shows a notification-listener form, answer from PLAY8 | merged manifest (9 permissions, verified by `verify-release-bundle.sh`) |
| Financial features | **No** | |
| Health | **No** | |
| Government app | **No** | |
| News app | **No** | |

### Target audience — operator decision (does not block building)

This can't be inferred safely. The product is a utility for connecting your own
computer and phone. It reads other apps' notifications (with consent), and
nothing about it is designed for children. **Recommendation: 18 and over.** It
is the truthful fit for the product. It also keeps the app out of the Families
policy, whose extra requirements a notification-reading utility has no reason
to take on. 13+ is defensible but adds nothing. Do not include children.

---

## PLAY14 — developer verification and package registration

> **Superseding note — 2026-09-24, Pliwee rebrand Wave 6 (branch `feature/pliwee-rebrand-wave6`), [ADR-0020](../../adr/ADR-0020-rename-to-pliwee.md) §D1.** The package to register is now **`io.github.yurisismotto.pliwee`**, and the bundle is verified to carry exactly that package (`android/signing/verify-release-bundle.sh`). The `io.github.yurisismotto.omnibridge` requirement below was never registered on Play and is retired. ADR-0020 sets the order — identity decided, implemented and certified, then the Play app created — and the creation belongs to the Play release wave (W10). The text below is left as it was written.

From 2026-09-30 every package on Play must be registered. For a Play developer:

* **Identity:** existing Play Console identity verification counts. Check under
  **Settings › Developer account**.
* **Package:** creating the app in Play Console registers the package name.
  Check the notice on **Home**, then **Android developer verification** ›
  **Package names**.
* The package **must be** `io.github.yurisismotto.omnibridge`. Play Console
  takes the package from the first uploaded bundle and never allows it to
  change. The bundle is verified to carry exactly this package.
* Under model B, the GitHub-release signing key *is* the app signing key, so
  the same certificate covers both channels. No extra key needs registering on
  the Package names tab (⚖ confirm in console).

This becomes an operator action at PLAY18, when the app is created.

## PLAY15 — testing requirement

Personal accounts created after 2023-11-13 must run a **closed test with ≥ 12
opted-in testers for 14 continuous days** before applying for production. The
account's type and creation date are **not known here** and have not been
assumed. Until the operator says otherwise, the plan is:

1. **Internal testing** first: upload, pre-launch report, PEPK and upload-key
   registration, fingerprints checked against ADR-0019.
2. If the rule applies: a **closed test**, 12+ testers opted in via the opt-in
   link, a 14-day gate tracked from the day the twelfth tester opts in (keep
   at least 12 opted in for the whole period — Play counts continuous
   opt-in), then **Dashboard › Apply for
   production access**.
3. Otherwise: production rollout after the internal test, at the operator's
   press.
