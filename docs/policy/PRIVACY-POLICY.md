# Pliwee Privacy Policy

**Effective:** 2026-10-06

**Applies to:** the Pliwee Android app (package `io.github.yurisismotto.pliwee`)
and the Pliwee desktop software it connects to.

Pliwee connects your Android device to computers you own, directly over your
local network, so you can move files and clipboard text between them, see your
phone's notifications on your computer, and see each device's battery on the
other. This policy says exactly what the app touches, where it goes, and how to
stop it.

## The short version

* **There is no Pliwee server.** No account, no sign-in, no cloud sync. The
  developer of Pliwee never receives any of your data, because there is no
  server, service or address for the app to send it to.
* **No advertising, analytics, telemetry or crash reporting.** The app includes
  no advertising or analytics SDK and makes no internet request of its own.
* **Data goes only to computers you pair.** You pair a computer by scanning a
  QR code it shows you. From then on the two talk directly on your local
  network over an encrypted connection that only that computer can answer.
* **Every kind of data has its own switch**, per computer, and can be turned
  off at any time. Clipboard and notifications are off until you turn them on.

## Who is responsible

**Developer:** Yuri C. Sismotto, who develops and publishes Pliwee and is
responsible for this app and this policy.

**Source code:** <https://github.com/yurisismotto/pliwee>

**Privacy contact:** for any question or request about this policy or about
your data, open an issue at <https://github.com/yurisismotto/pliwee/issues>.
Issues are public, so do not include personal information in them.

## Three places your data can be

This policy keeps three things apart:

1. **Processed on your phone.** Most of what the app touches is read and used
   on your phone and never leaves it: the camera image while you scan, your
   list of paired computers, your settings, your device key.
2. **Sent directly to a computer you paired.** File, clipboard, notification
   and battery data leave your phone only when that kind of data is allowed for
   that computer, and only to that computer, over your local network. The
   Pliwee desktop software on that computer is run by you, not by the developer.
3. **Received by the developer: nothing.** There is no Pliwee backend, cloud
   service or server. Nothing described in this policy is sent to the
   developer or to any third party.

## What Pliwee uses, and why

### Pairing and device identity

When you first pair, the app creates a cryptographic key in your device's
Android Keystore (hardware-backed where the device supports it). The private
key can't be exported and never leaves the device. When your phone connects to
a paired computer it tells that computer:

* a random device identifier created by the app. It is not your Android ID,
  your IMEI, your advertising ID or any other hardware identifier;
* your device's name, which is its model name (for example "SM-X620");
* that it is an Android device;
* the public fingerprint of its key;
* which Pliwee features and protocol versions it supports.

The computer tells your phone the same kinds of thing about itself: its own
random identifier, its name (by default the computer's hostname), its key
fingerprint and the features it supports.

To find a paired computer on your network, the app listens for the
announcements that the Pliwee desktop software makes on the local network
(multicast DNS). Your phone only listens; it does not announce itself. The
desktop's announcement carries the computer's identifier and name, and can be
turned off on the computer.

On your phone the app keeps a list of the computers you have paired: each
one's name, identifier, key fingerprint, when you paired it, which features you
have allowed it, your settings for it, and up to four local network addresses
it was last reached at. The computer keeps the matching record about your
phone: its name, identifier, platform, key fingerprint, when it was paired and
what it is allowed.

### Camera — only to scan the pairing code

The camera is used only to scan the QR code a computer shows when you pair.
Android asks for camera permission only when you tap **Pair**. The image is
decoded on your device and discarded. No photo or video is saved or sent. Only
the decoded pairing code is used, and only to reach that computer.

### Clipboard

* **Phone → computer: only when you choose to send.** You can send from the
  Send screen, the Quick Settings tile, or by sharing text to Pliwee. The app
  never watches your clipboard and never sends it on its own. If the clipboard
  is marked sensitive (for example a copied password), the app does not show
  it and asks you again before sending.
* **Computer → phone:** text a paired computer sends is held in memory for at
  most five minutes until you tap to copy it. If you turn on automatic receive
  for that computer, it is copied straight away.
* What is sent: the text (up to 32 KiB), a checksum of it, whether it is marked
  sensitive, and a timestamp.
* Clipboard text is never written to your device's storage, and the Pliwee
  desktop software does not write it to disk either. It goes only into the
  receiving device's clipboard.

Clipboard sharing is **off** for each computer until you turn it on.

### Files

* **Phone → computer:** only files you pick in Android's file picker or share
  to Pliwee. Android gives the app access to that one file for that one
  transfer. Pliwee asks for no storage permission.
* **Computer → phone:** you are asked to accept every incoming file. Accepted
  files are saved to `Download/Pliwee` on your device. They are yours and stay
  there, even if you uninstall the app.
* What is sent with a file: its name, size, type and a checksum.
* Files you send to a computer are saved in a `Pliwee` folder inside that
  computer's Downloads folder.
* The app keeps no history of transfers. The list on screen lasts only while
  the app is running.

Files are allowed when you pair a computer, because every incoming file still
needs your approval. You can turn files off for any computer.

### Notifications (optional, off by default)

If you turn it on, Pliwee can show your phone's notifications on your computer.
Three separate things must all be true before anything is sent:

1. **You allow that computer.** Turning this on shows a screen explaining what
   will be sent, and nothing is allowed until you tap **Allow**.
2. **You allow Android notification access** for Pliwee in Android's own
   settings.
3. **You choose which apps.** No app is chosen for you.

For each notification from an app you chose, the computer receives:

* the app's name and package name;
* the notification's title and text;
* when it was posted, its category, importance, lock-screen visibility and
  progress;
* whether it is ongoing, can be dismissed, summarises a group, or comes from a
  work profile;
* a random-looking identifier for the notification and a checksum of its
  content, so the computer can update or remove it.

It never receives pictures, icons, action buttons, reply fields or the people
attached to a notification. Pliwee never shares its own notifications, or
notifications an app has marked as secret on the lock screen. Ongoing
notifications and notifications from a work profile are not shared unless you
turn them on for that computer.

**While your phone is locked**, only the app's name is sent by default. You can
change this to send everything or nothing.

**When it runs:** Pliwee sends notifications only while your phone is connected
to a computer you allowed for this. That can happen while the app is in the
background. Pliwee asks Android to attach its notification reader only while
such a computer is connected, and to detach it afterwards. One exception has
been observed: if Pliwee is force-stopped while attached, Android may re-attach
the reader and keep it attached until notification access is turned off and on
again. While no allowed computer is connected, nothing is sent anywhere.

Notifications are not stored on your phone. The Pliwee desktop software shows
them through your computer's own notification system and does not save them;
your desktop environment may keep them in its notification list until you
dismiss them.

If you turn on **dismiss sync** for a computer, dismissing a notification on
that computer dismisses it on your phone. This is off by default, and it never
applies to ongoing notifications.

The apps you chose, and the list of apps you were last shown to pick from, are
stored on your phone with that computer's settings. That list is not sent to
the computer.

### Battery

Your phone's battery percentage and charging state are sent to a paired
computer when it connects, so the computer can show them. This is allowed when
you pair, and you can turn it off per computer with the **Battery** switch. A
laptop's battery reading is sent to your phone and shown there. Neither device
stores the other's battery readings; they are kept only in memory.

### Background connection (foreground service)

While connected, Pliwee runs a foreground service of type "connected device" so
the link to your computer stays up with the screen off. It starts only when you
pair or connect, or when you share something to a computer. It never starts
when the phone boots. It stops when you tap **Disconnect**, revoke the
computer, or the connection gives up. While it runs, Pliwee appears in
Android's list of active apps and shows an ongoing notification if you allow
notifications.

### Notification permission

Pliwee asks to post notifications when you first connect. It uses them for the
connection status and for "clipboard received" prompts, which show the
computer's name and the size, never the text. Everything else works without
this permission.

## How your data is protected in transit

Everything between your phone and a computer goes over **TLS 1.3**, and each
side proves its identity with its own key. Your phone accepts only the exact
computer whose fingerprint it learned when you paired. There is no certificate
authority to trick, and no unencrypted connection is ever allowed. Pairing
itself is protected by a one-time code in the QR code, which both sides must
prove they know. Pliwee does not work over the internet or mobile data; it is
built for devices on the same local network.

## What stays on your phone

* Your device key and a second key used to derive notification identifiers,
  both in the Android Keystore and not exportable.
* The paired-computer list and your settings for each computer, described
  above, in the app's private storage.
* Files you accepted, in `Download/Pliwee`.

App data is excluded from Android cloud backup and from device-to-device
transfer, so it is never copied to Google's servers or to a new phone.

## Technical logs

Pliwee writes technical events to the Android system log on your phone, such as
connection attempts, shortened transfer identifiers, sizes, network addresses
and shortened key fingerprints. That log never contains clipboard text,
notification content or file names. It stays on your device and Pliwee sends it
nowhere.

The Pliwee desktop software writes similar events to your computer's system
log. On the computer, that log does include the names of files sent to or
received from your phone, as well as network addresses and device
identifiers. It never contains clipboard text or notification content, and
Pliwee sends it nowhere.

## Sharing with third parties

Pliwee does not sell, rent or share your data with anyone, and has no third
party to share it with: no advertising network, analytics provider or cloud
host. The only place anything goes is a computer you paired yourself, and you
choose, per computer, what it may receive.

## Your choices, and how to stop

| To stop… | Do this |
| --- | --- |
| one kind of data to one computer | open the computer in **Devices** and turn off Clipboard, Files or Battery; for notifications, turn off **Share notifications with this computer** |
| all notification reading | turn off Pliwee in Android's **Notification access** settings |
| everything to one computer | **Revoke this device** on its card |
| the current connection | **Disconnect** |
| everything, everywhere | uninstall Pliwee, or clear its storage in Android settings |

Camera and notification permissions can also be withdrawn at any time in
Android's app settings.

### Revoking and removing a computer

**Revoke this device** takes effect on your phone at once: the connection is
closed, every permission and setting for that computer is erased, its saved
network addresses are forgotten, and your phone refuses to connect to it
again. Nothing is sent to the computer to announce it; it simply can no longer
connect. You can then **Remove from list**, which erases the computer's name,
identifier and pairing date and keeps only its key fingerprint, marked as
revoked, so that the same key is not silently accepted again.

A computer can also unpair your phone from its own side. The phone then can no
longer connect to it.

### Deleting your data

* **Data on your phone:** clear Pliwee's storage in Android settings, or
  uninstall the app. Either deletes the device key, the notification key, the
  paired-computer list and all settings. You will need to pair again.
* **Files you received** stay in `Download/Pliwee` until you delete them,
  because they are your files, not the app's.
* **Data on your computer:** the Pliwee desktop software keeps its record of
  your phone, and any files you sent, on your computer. Unpair the phone there,
  delete the files, or uninstall the desktop software.

### Uninstalling

Uninstalling the app deletes everything it stored on your phone, as Android
removes an app's private storage and keys with the app. Files you accepted, in
`Download/Pliwee`, are not deleted. Nothing needs to be deleted anywhere else,
because the developer holds none of your data.

## Children

Pliwee is a tool for connecting your own devices. It is not directed at
children, and it does not knowingly collect any data from anyone, child or
adult.

## Changes

Changes to this policy are published at this address, with a new effective
date. Because the policy lives beside the source code, every change is recorded
in the project's public history at <https://github.com/yurisismotto/pliwee>.
