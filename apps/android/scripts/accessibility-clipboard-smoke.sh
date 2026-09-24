#!/usr/bin/env bash
set -euo pipefail

ADB="${ADB:-adb}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ANDROID_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
APP_APK="$ANDROID_DIR/app/build/outputs/apk/debug/app-debug.apk"
TEST_APK="$ANDROID_DIR/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk"
COMPONENT="app.span.android/app.span.android.SpanKeepAliveService"
FIXTURE="app.span.android.AccessibilityClipboardFixtureTest"
PROBE="app.span.android.test/app.span.android.ClipboardProbeActivity"
CAPTURE_FILE="$(mktemp "${TMPDIR:-/tmp}/span-android-send.XXXXXX")"
LISTENER_PID=""

cleanup() {
  "$ADB" forward --remove tcp:46793 >/dev/null 2>&1 || true
  "$ADB" shell settings put secure accessibility_enabled 0 >/dev/null 2>&1 || true
  "$ADB" shell settings delete secure enabled_accessibility_services >/dev/null 2>&1 || true
  if [[ -n "$LISTENER_PID" ]]; then
    kill "$LISTENER_PID" >/dev/null 2>&1 || true
  fi
  rm -f "$CAPTURE_FILE"
}
trap cleanup EXIT

fail() {
  printf 'Accessibility clipboard smoke test failed: %s\n' "$1" >&2
  "$ADB" logcat -d -s SpanReceiveService:V SpanClipboardProbe:V AndroidRuntime:E '*:S' >&2 || true
  exit 1
}

wait_for_text() {
  local command="$1"
  local expected="$2"
  local attempts="${3:-50}"
  local output=""
  for ((index = 0; index < attempts; index++)); do
    output="$(eval "$command")"
    if grep -Fq "$expected" <<<"$output"; then
      return 0
    fi
    sleep 0.2
  done
  return 1
}

[[ -f "$APP_APK" ]] || fail "missing $APP_APK"
[[ -f "$TEST_APK" ]] || fail "missing $TEST_APK"

"$ADB" install -r "$APP_APK" >/dev/null
"$ADB" install -r "$TEST_APK" >/dev/null

fixture_output="$("$ADB" shell am instrument -w -r \
  -e class "$FIXTURE" \
  app.span.android.test/androidx.test.runner.AndroidJUnitRunner)"
grep -Fq 'OK (1 test)' <<<"$fixture_output" || fail "fixture instrumentation failed"

# Instrumentation force-stops the target app. Toggle both settings afterwards
# so AccessibilityManager creates a fresh production service connection.
"$ADB" shell settings put secure accessibility_enabled 0 >/dev/null
"$ADB" shell settings delete secure enabled_accessibility_services >/dev/null || true
"$ADB" shell settings put secure enabled_accessibility_services "$COMPONENT" >/dev/null
"$ADB" shell settings put secure accessibility_enabled 1 >/dev/null
wait_for_text \
  "'$ADB' shell dumpsys accessibility" \
  'Bound services:{Service[label=Span' \
  || fail "Span accessibility service did not bind"

"$ADB" shell pm grant app.span.android android.permission.POST_NOTIFICATIONS >/dev/null 2>&1 || true
"$ADB" shell am start -W -n app.span.android/.MainActivity >/dev/null
"$ADB" shell am force-stop app.span.android.test
"$ADB" shell am start -W -n "$PROBE" --es set_value span-test-baseline >/dev/null
"$ADB" logcat -c

# PC -> Android: send the Rust compatibility vector through the production TCP
# receiver while another app owns the foreground. The accessibility overlay
# must write it without launching any Span Activity.
"$ADB" forward tcp:46793 tcp:46793 >/dev/null
printf '%s\t%s\t%s\t%s\n' \
  'SPAN_TEXT_V3' \
  'rust-test-pc' \
  'fe73b77324bf874d744d4bc6' \
  '3d5f060122a3ad13c0b30bd288e941ac9f2e4ebe0912c114459e098d00076719fc3060f3fd4b43b430dcaceb6d87b4b4' \
  > /dev/tcp/127.0.0.1/46793
wait_for_text \
  "'$ADB' logcat -d -s SpanClipboardProbe:I '*:S'" \
  'clipboard=Rust PC to Android clipboard' \
  || fail "PC text did not reach the foreground clipboard target"
"$ADB" forward --remove tcp:46793 >/dev/null

top_activity="$("$ADB" shell dumpsys activity activities \
  | grep -m1 -E 'topResumedActivity=|mResumedActivity:|ResumedActivity:' || true)"
grep -Fq "$PROBE" <<<"$top_activity" || fail "PC receive opened a Span Activity"
for ((index = 0; index < 50; index++)); do
  preferences="$("$ADB" exec-out run-as app.span.android cat shared_prefs/span.xml)"
  ! grep -Fq 'clipboard.pending_remote_text' <<<"$preferences" && break
  sleep 0.2
done
if grep -Fq 'clipboard.pending_remote_text' <<<"$preferences"; then
  fail "verified remote clipboard remained pending"
fi

# Android -> PC: keep the probe Activity in front, seed a local clipboard value,
# then invoke the same service action used by the notification. A host listener
# must receive an encrypted Span packet while the probe remains top-resumed.
"$ADB" shell am force-stop app.span.android.test
"$ADB" shell am start -W -n "$PROBE" --es set_value android-outbound-clipboard-test >/dev/null
wait_for_text \
  "'$ADB' logcat -d -s SpanClipboardProbe:I '*:S'" \
  'clipboard=android-outbound-clipboard-test' \
  || fail "could not seed Android clipboard"

nc -l 46793 >"$CAPTURE_FILE" &
LISTENER_PID="$!"
sleep 0.2
"$ADB" shell run-as app.span.android am startservice --user 0 \
  -a app.span.android.action.SEND_CLIPBOARD \
  -n app.span.android/.SpanReceiveService >/dev/null

for ((index = 0; index < 50; index++)); do
  [[ -s "$CAPTURE_FILE" ]] && break
  sleep 0.2
done
[[ -s "$CAPTURE_FILE" ]] || fail "Android accessibility send produced no TCP packet"
wait "$LISTENER_PID" || true
LISTENER_PID=""
grep -Fq $'SPAN_TEXT_V3\tandroid-test\t' "$CAPTURE_FILE" \
  || fail "Android accessibility send produced an invalid packet"

top_activity="$("$ADB" shell dumpsys activity activities \
  | grep -m1 -E 'topResumedActivity=|mResumedActivity:|ResumedActivity:' || true)"
grep -Fq "$PROBE" <<<"$top_activity" || fail "Android send opened a Span Activity"
if "$ADB" logcat -d -s AndroidRuntime:E '*:S' \
    | grep -A1 'FATAL EXCEPTION' \
    | grep -Fq 'Process: app.span.android'; then
  fail "Span crashed during accessibility clipboard transfer"
fi

printf 'Accessibility clipboard smoke test passed in both directions.\n'
