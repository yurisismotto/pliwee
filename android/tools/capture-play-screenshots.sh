#!/usr/bin/env bash
# Captures the Google Play listing screenshots from the real app UI.
#
#   android/tools/capture-play-screenshots.sh phone|tablet
#   android/tools/capture-play-screenshots.sh verify
#
# `verify` boots nothing: it checks the eight committed assets (PNG, exact
# size, 9:16, no alpha channel) and is what to run before a commit.
#
# Boots a dedicated emulator, installs the debug app and its androidTest APK,
# and runs PlayStoreScreenshots, which renders the real PliweeShell from a
# fictional state ("Linux Desktop", no addresses, a made-up fingerprint) and
# captures each screen with screencap. Nothing touches a real device: the
# script refuses to run if the AVD's serial is already taken, and it talks to
# that serial only.
#
# Profiles:
#   phone   AVD pliwee-phone  (Pixel 8, display set to 1080x1920, 420 dpi),
#           captured 1080x1920 portrait, smallest width ~411 dp: a phone
#   tablet  AVD pliwee-tablet (Pixel Tablet, display set to 2560x1440,
#           320 dpi), captured 1440x2560 portrait, smallest width 720 dp:
#           still a tablet to Android, which the script checks
#
# Both are 9:16. The phone is not the Pixel 8's native 1080x2400 because Play
# rejects a screenshot whose long side exceeds twice the short one; the tablet
# is not the Pixel Tablet's native 2560x1600 because Play recommends 9:16 for
# large-screen screenshots. Only the panel's pixel count changes; density
# stays the device's, and the app lays itself out for the real size. The
# tablet is captured in portrait: the shell caps content at 640 dp, so in
# landscape it is a phone column between two empty margins.
#
# Every capture must be fully opaque, and is then stored as 24-bit RGB without
# an alpha channel (Play does not accept alpha in screenshots). The script
# compares the result with the capture and refuses it if one pixel differs.
#
# One-time setup (API 36 is the app's compileSdk and targetSdk):
#   sdkmanager "emulator" "system-images;android-36;google_apis;x86_64"
#   avdmanager create avd -n pliwee-phone  -k "system-images;android-36;google_apis;x86_64" -d pixel_8
#   avdmanager create avd -n pliwee-tablet -k "system-images;android-36;google_apis;x86_64" -d pixel_tablet
#   in ~/.android/avd/pliwee-phone.avd/config.ini:  hw.lcd.height = 1920
#   in ~/.android/avd/pliwee-tablet.avd/config.ini: hw.lcd.height = 1440
#
# Build first:
#   (cd android && ./gradlew :app:assembleDebug :app:assembleDebugAndroidTest)
#
# Environment: ANDROID_HOME (default ~/Android/Sdk), AVD_NAME to override the
# profile's AVD, KEEP_EMULATOR=1 to leave it running.
set -euo pipefail

die() { echo "capture-play-screenshots: $*" >&2; exit 1; }

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
android_dir="$(dirname "$here")"
repo="$(dirname "$android_dir")"
shots=(01-devices 02-device-details 03-clipboard 04-files-or-notifications)

command -v file >/dev/null || die "'file' is required to validate the PNGs"
command -v magick >/dev/null || die "ImageMagick 'magick' is required to check and store the PNGs"

# One final asset: a PNG of exactly w x h, 9:16, 24-bit RGB with no alpha.
check_asset() {
    local path="$1" w="$2" h="$3" info
    [[ -s "$path" ]] || die "$path is missing or empty"
    info="$(file -b "$path")"
    grep -q "^PNG image data, $w x $h, 8-bit/color RGB," <<<"$info" \
        || die "$path is not a ${w}x${h} 24-bit RGB PNG without alpha: $info"
    (( w * 16 == h * 9 )) || die "$path is ${w}x${h}, not 9:16"
    echo "   $path  ($info, $(stat -c %s "$path") bytes)"
}

profile="${1:-}"
case "$profile" in
    phone)  avd="${AVD_NAME:-pliwee-phone}";  port=5580; rotation=0; want_w=1080; want_h=1920
            panel=1080x1920; sw_check='sw < 600' ;;
    tablet) avd="${AVD_NAME:-pliwee-tablet}"; port=5582; rotation=1; want_w=1440; want_h=2560
            panel=2560x1440; sw_check='sw >= 600' ;;
    verify)
        for shot in "${shots[@]}"; do
            check_asset "$repo/docs/design/assets/play/screenshots/phone/$shot.png" 1080 1920
            check_asset "$repo/docs/design/assets/play/screenshots/tablet/$shot.png" 1440 2560
        done
        for dir in phone tablet; do
            found="$(find "$repo/docs/design/assets/play/screenshots/$dir" -type f | sort)"
            expected="$(printf "$repo/docs/design/assets/play/screenshots/$dir/%s.png\n" "${shots[@]}" | sort)"
            [[ "$found" == "$expected" ]] || die "$dir/ must hold exactly the four assets; found: $(tr '\n' ' ' <<<"$found")"
        done
        echo "== verify: 8 assets, all 9:16, no alpha"
        exit 0 ;;
    *) die "usage: $0 phone|tablet|verify" ;;
esac
(( want_w * 16 == want_h * 9 )) || die "profile $profile is not 9:16"

out="$repo/docs/design/assets/play/screenshots/$profile"
app_apk="$android_dir/app/build/outputs/apk/debug/app-debug.apk"
test_apk="$android_dir/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk"
sdk="${ANDROID_HOME:-$HOME/Android/Sdk}"
emulator="$sdk/emulator/emulator"
adb="$sdk/platform-tools/adb"
serial="emulator-$port"
remote=/data/local/tmp/pliwee-play-screenshots

# --- preconditions: every tool, every input -------------------------------
[[ -x "$emulator" ]] || die "no emulator at $emulator (sdkmanager \"emulator\")"
[[ -x "$adb" ]] || die "no adb at $adb (sdkmanager \"platform-tools\")"
[[ -s "$app_apk" ]] || die "missing $app_apk; build :app:assembleDebug first"
[[ -s "$test_apk" ]] || die "missing $test_apk; build :app:assembleDebugAndroidTest first"
avds="$("$emulator" -list-avds)"
[[ -n "$avds" ]] || die "no AVDs exist; create $avd with avdmanager"
grep -qx "$avd" <<<"$avds" || die "AVD '$avd' not found; have: $(tr '\n' ' ' <<<"$avds")"
devices="$("$adb" devices)"
if grep -q "^$serial" <<<"$devices"; then
    die "$serial is already attached; stop it first so this run owns its emulator"
fi

echo "== $profile: AVD $avd on $serial"
"$emulator" -avd "$avd" -port "$port" -no-window -no-audio -no-boot-anim \
    -no-snapshot -gpu swangle_indirect >"${TMPDIR:-/tmp}/pliwee-emulator-$profile.log" 2>&1 &
emu_pid=$!
cleanup() {
    if [[ "${KEEP_EMULATOR:-0}" != 1 ]]; then
        "$adb" -s "$serial" emu kill >/dev/null 2>&1 || kill "$emu_pid" 2>/dev/null || true
        # Gone, not just asked to go, so a following run finds the port free.
        wait "$emu_pid" 2>/dev/null || true
    fi
}
trap cleanup EXIT

a() { "$adb" -s "$serial" "$@"; }

# --- wait for Android, not just for adbd -----------------------------------
timeout 120 "$adb" -s "$serial" wait-for-device || die "$serial never appeared (see emulator log)"
for _ in $(seq 1 180); do
    kill -0 "$emu_pid" 2>/dev/null || die "emulator exited during boot"
    booted="$(a shell getprop sys.boot_completed 2>/dev/null | tr -d "\r" || true)"
    [[ "$booted" == 1 ]] && break
    sleep 2
done
[[ "${booted:-}" == 1 ]] || die "$serial did not finish booting in 6 minutes"
pm_out="$(a shell pm path android | tr -d '\r')"
[[ -n "$pm_out" ]] || die "package manager not ready on $serial"
echo "== booted: $(a shell getprop ro.build.version.release | tr -d '\r') / API $(a shell getprop ro.build.version.sdk | tr -d '\r')"

# The device class is measured, not assumed from the AVD's name: the panel
# must be the one configured, and its smallest width on the right side of
# Android's 600 dp phone/tablet line.
size="$(a shell wm size | tr -d '\r')"
grep -q "^Physical size: $panel$" <<<"$size" || die "expected a $panel panel; wm size says: $size"
config="$(a shell am get-config | tr -d '\r')"
# The qualifier string, e.g. "config: mcc310-...-sw720dp-w1280dp-...".
sw="$(grep -o -m1 -e '-sw[0-9]*dp-' <<<"$config" | tr -dc '0-9' || true)"
[[ -n "$sw" ]] || die "no smallest width in am get-config"
(( $sw_check )) || die "smallest width ${sw}dp fails '$sw_check' for $profile"
echo "== display: $panel, $(a shell wm density | tr -d '\r' | head -1), smallest width ${sw}dp"

# --- a quiet, fixed-orientation screen ------------------------------------
a shell settings put global window_animation_scale 0
a shell settings put global transition_animation_scale 0
a shell settings put global animator_duration_scale 0
a shell settings put system pointer_location 0
a shell settings put system show_touches 0
a shell cmd statusbar collapse >/dev/null 2>&1 || true

# --- install and capture ---------------------------------------------------
a install -r -t "$app_apk" >/dev/null || die "installing $app_apk failed"
a install -r -t "$test_apk" >/dev/null || die "installing $test_apk failed"

# Locked here, last, and confirmed: on a fresh API 36 image something resets
# the rotation to "free" some time after boot_completed, so a lock taken at
# boot was observed to be gone by the time of the capture.
rotated=""
for _ in $(seq 1 15); do
    a shell settings put system accelerometer_rotation 0
    a shell wm user-rotation lock "$rotation" >/dev/null
    sleep 2
    windows="$(a shell dumpsys window | tr -d '\r')"
    if grep -q "mRotation=$rotation " <<<"$windows"; then rotated=yes; break; fi
done
[[ "$rotated" == yes ]] || die "display did not rotate to $rotation"

# Demo status bar, once the emulator's own network has settled. Applied
# earlier, it raced the real Wi-Fi connecting: phone captures came out with
# two Wi-Fi icons, the mobile signal, or a satellite icon. Demo state also
# survives `demo exit`, so it is applied exactly once per boot.
wifi=""
for _ in $(seq 1 60); do
    status="$(a shell cmd wifi status 2>/dev/null | tr -d '\r' || true)"
    if grep -q 'Wifi is connected' <<<"$status"; then wifi=yes; break; fi
    sleep 2
done
[[ "$wifi" == yes ]] || die "the emulator's Wi-Fi never connected; the status bar would not settle"
sleep 15
a shell settings put global sysui_demo_allowed 1
demo() { a shell am broadcast -a com.android.systemui.demo -e command "$@" >/dev/null; }
demo enter
sleep 1
demo clock -e hhmm 1000
demo battery -e level 100 -e plugged false -e powersave false
demo network -e wifi show -e level 4 -e fully true
demo network -e mobile hide
demo network -e satellite hide
demo network -e airplane hide
demo network -e nosim hide
demo notifications -e visible false
demo status -e volume hide -e bluetooth hide -e location hide -e alarm hide -e mute hide

# Two observations around the run: the directory is gone before, and holds
# exactly the four new captures after. A skipped test leaves nothing behind.
a shell rm -rf "$remote"
before="$(a shell ls "$remote" 2>&1 | tr -d "\r" || true)"
grep -q 'No such file' <<<"$before" || die "could not clear $remote: $before"

result="$(a shell am instrument -w -r \
    -e playScreenshots true \
    -e class io.github.yurisismotto.pliwee.PlayStoreScreenshots \
    io.github.yurisismotto.pliwee.test/androidx.test.runner.AndroidJUnitRunner | tr -d '\r')"
grep -q '^OK (1 test)' <<<"$result" || { echo "$result" >&2; die "instrumentation did not pass"; }
if grep -q 'AssumptionViolated\|Ignored' <<<"$result"; then
    echo "$result" >&2; die "the capture was skipped, not run"
fi

listing="$(a shell ls "$remote" | tr -d '\r' | sort)"
expected="$(printf '%s.png\n' "${shots[@]}" | sort)"
[[ "$listing" == "$expected" ]] || die "expected exactly: $(tr '\n' ' ' <<<"$expected"); got: $(tr '\n' ' ' <<<"$listing")"

mkdir -p "$out"
for shot in "${shots[@]}"; do
    dest="$out/$shot.png"
    rm -f "$dest"
    raw="${TMPDIR:-/tmp}/pliwee-$profile-$shot.raw.png"
    rm -f "$raw"
    a pull "$remote/$shot.png" "$raw" >/dev/null 2>&1 || die "pull of $shot failed"
    [[ -s "$raw" ]] || die "$raw is empty"
    raw_info="$(file -b "$raw")"
    grep -q "^PNG image data, $want_w x $want_h," <<<"$raw_info" \
        || die "$shot was captured as $raw_info, not ${want_w}x${want_h}"
    opaque="$(magick identify -format '%[opaque]' "$raw")"
    [[ "$opaque" == True ]] || die "$shot has transparent pixels; dropping alpha would change it"
    magick "$raw" -alpha off -strip -define png:color-type=2 "$dest"
    # ImageMagick 7 prints "0 (0)": the absolute count, then normalised.
    diff_px="$(magick compare -metric AE "$raw" "$dest" null: 2>&1 || true)"
    diff_px="${diff_px%% *}"
    [[ "$diff_px" == 0 ]] || die "$shot changed when alpha was dropped ($diff_px pixels)"
    rm -f "$raw"
    check_asset "$dest" "$want_w" "$want_h"
done
a shell rm -rf "$remote"
demo exit
echo "== $profile: ${#shots[@]} screenshots in $out"
