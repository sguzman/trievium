#!/usr/bin/env bash
# Verify Trievium receives a real keyboard event inside an isolated Wayland
# compositor, copies sample text and exits. Never inject host keyboard events.
set -euo pipefail

export XDG_RUNTIME_DIR="${RUNNER_TEMP:-/tmp}/trievium-wayland-runtime"
mkdir -p "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR"
unset DISPLAY || true
export WLR_BACKENDS=headless
export WLR_HEADLESS_OUTPUTS=1
export WLR_LIBINPUT_NO_DEVICES=1
export WLR_RENDERER=pixman
export WLR_RENDERER_ALLOW_SOFTWARE=1
export LIBGL_ALWAYS_SOFTWARE=1
export XDG_CURRENT_DESKTOP=sway

sway_pid=""
keyboard_pid=""
picker_pid=""
cleanup() {
  for pid in "$picker_pid" "$keyboard_pid" "$sway_pid"; do
    if [[ -n "$pid" ]]; then
      kill "$pid" 2>/dev/null || true
    fi
  done
}
trap cleanup EXIT

cat > "$XDG_RUNTIME_DIR/sway.conf" <<'CONF'
output * resolution 1280x720
default_border none
focus_follows_mouse no
CONF

sway --unsupported-gpu -c "$XDG_RUNTIME_DIR/sway.conf" >"$XDG_RUNTIME_DIR/sway.log" 2>&1 &
sway_pid=$!
socket=""
for _ in $(seq 1 100); do
  socket=$(find "$XDG_RUNTIME_DIR" -maxdepth 1 -name 'wayland-*' -type s -print -quit)
  if [[ -n "$socket" ]]; then
    break
  fi
  sleep 0.1
done
if [[ -z "$socket" ]]; then
  echo "No Wayland socket from nested Sway" >&2
  cat "$XDG_RUNTIME_DIR/sway.log" >&2
  exit 1
fi
export WAYLAND_DISPLAY="$(basename "$socket")"
sway_sock=$(find "$XDG_RUNTIME_DIR" -maxdepth 1 -name 'sway-ipc.*.sock' -type s -print -quit)
if [[ -z "$sway_sock" ]]; then
  echo "No nested sway IPC socket" >&2
  cat "$XDG_RUNTIME_DIR/sway.log" >&2
  exit 1
fi

# A persistent virtual keyboard keeps seat keyboard capability advertised to
# winit before Trievium connects. F15 is inert; the long delay keeps it alive.
wtype -s 999999999 -k F15 -k F15 >"$XDG_RUNTIME_DIR/virtual-keyboard.log" 2>&1 &
keyboard_pid=$!
sleep 0.3

config="$XDG_RUNTIME_DIR/sample-items.json"
./target/debug/trievium init --config "$config"
./target/debug/trievium --config "$config" --diagnose-input >"$XDG_RUNTIME_DIR/trievium.log" 2>&1 &
picker_pid=$!

found=""
for _ in $(seq 1 100); do
  found=$(swaymsg -s "$sway_sock" -t get_tree | grep -o '"app_id": "trievium"' | head -1 || true)
  if [[ -n "$found" ]]; then
    break
  fi
  sleep 0.1
done
if [[ -z "$found" ]]; then
  echo "Picker did not map to nested Sway" >&2
  cat "$XDG_RUNTIME_DIR/trievium.log" "$XDG_RUNTIME_DIR/sway.log" >&2
  exit 1
fi
swaymsg -s "$sway_sock" '[app_id="trievium"] focus'
sleep 0.3
wtype g

copied=""
for _ in $(seq 1 70); do
  copied=$(wl-paste -n 2>/dev/null || true)
  if [[ "$copied" == "Hello!" ]]; then
    break
  fi
  sleep 0.1
done
if [[ "$copied" != "Hello!" ]]; then
  echo "Real Wayland keyboard input did not select the G shortcut" >&2
  cat "$XDG_RUNTIME_DIR/trievium.log" "$XDG_RUNTIME_DIR/virtual-keyboard.log" >&2
  exit 1
fi
# Clipboard success alone is insufficient: the original regression left
# a popup alive after a completed selection. Require the one-shot process to
# terminate before relaunching with a different key.
wait_for_picker_exit() {
  local state=""
  for _ in $(seq 1 70); do
    state=$(ps -o stat= -p "$picker_pid" 2>/dev/null | tr -d '[:space:]' || true)
    if [[ -z "$state" || "$state" == Z* ]]; then
      wait "$picker_pid"
      picker_pid=""
      return 0
    fi
    sleep 0.1
  done
  echo "Trievium copied text but did not exit" >&2
  cat "$XDG_RUNTIME_DIR/trievium.log" >&2
  return 1
}
wait_for_picker_exit

# Relaunch from the same session to catch focus handoff and stale single-
# instance-lock bugs. The next key must copy a *different* entry and exit.
./target/debug/trievium --config "$config" --diagnose-input >"$XDG_RUNTIME_DIR/trievium-second.log" 2>&1 &
picker_pid=$!
found=""
for _ in $(seq 1 100); do
  found=$(swaymsg -s "$sway_sock" -t get_tree | grep -o '"app_id": "trievium"' | head -1 || true)
  if [[ -n "$found" ]]; then
    break
  fi
  sleep 0.1
done
if [[ -z "$found" ]]; then
  echo "Second picker did not map after first selection" >&2
  cat "$XDG_RUNTIME_DIR/trievium-second.log" >&2
  exit 1
fi
swaymsg -s "$sway_sock" '[app_id="trievium"] focus'
sleep 0.3
wtype c
copied=""
for _ in $(seq 1 70); do
  copied=$(wl-paste -n 2>/dev/null || true)
  if [[ "$copied" == "Best regards," ]]; then
    break
  fi
  sleep 0.1
done
if [[ "$copied" != "Best regards," ]]; then
  echo "Second Wayland picker did not select C" >&2
  cat "$XDG_RUNTIME_DIR/trievium-second.log" >&2
  exit 1
fi
wait_for_picker_exit
echo "Wayland E2E passed: G and C copied different payloads, and both pickers exited."

# Regression for the unusable editor: start the full-sized management window
# and close it with Escape. It must not hang or mutate the user's data.
cp "$config" "$XDG_RUNTIME_DIR/original-config.json"
./target/debug/trievium manage --config "$config" >"$XDG_RUNTIME_DIR/manager.log" 2>&1 &
picker_pid=$!
found=""
for _ in $(seq 1 100); do
  found=$(swaymsg -s "$sway_sock" -t get_tree | grep -o '"app_id": "trievium"' | head -1 || true)
  if [[ -n "$found" ]]; then
    break
  fi
  sleep 0.1
done
if [[ -z "$found" ]]; then
  echo "Native management window did not map" >&2
  cat "$XDG_RUNTIME_DIR/manager.log" >&2
  exit 1
fi
swaymsg -s "$sway_sock" '[app_id="trievium"] focus'
sleep 0.2
wtype -k Escape
wait_for_picker_exit
cmp "$config" "$XDG_RUNTIME_DIR/original-config.json"
echo "Wayland manager E2E passed: Escape closed editor and preserved configuration."

# Shortest-unique-prefix regression. A reserved 'qz' must select after 'q'
# alone if no other key begins with q. A branch shared by 'ta' and 'tb'
# must wait for its disambiguating second letter. Unknown keys do nothing.
unique_config="$XDG_RUNTIME_DIR/unique-config.json"
cat > "$unique_config" <<'JSON'
{"version":1,"entries":[
  {"label":"Quick","text":"Unique prefix selected","key":"QZ"},
  {"label":"Closing","text":"Closing alternative","key":"C"}
]}
JSON
./target/debug/trievium --config "$unique_config" >"$XDG_RUNTIME_DIR/unique.log" 2>&1 &
picker_pid=$!
for _ in $(seq 1 100); do
  if swaymsg -s "$sway_sock" -t get_tree | grep -q '"app_id": "trievium"'; then
    break
  fi
  sleep 0.1
done
swaymsg -s "$sway_sock" '[app_id="trievium"] focus'
sleep 0.2
wtype q
copied=""
for _ in $(seq 1 70); do
  copied=$(wl-paste -n 2>/dev/null || true)
  if [[ "$copied" == "Unique prefix selected" ]]; then
    break
  fi
  sleep 0.1
done
if [[ "$copied" != "Unique prefix selected" ]]; then
  echo "Single unique prefix did not copy without remaining reserved letters" >&2
  cat "$XDG_RUNTIME_DIR/unique.log" >&2
  exit 1
fi
wait_for_picker_exit

ambiguous_config="$XDG_RUNTIME_DIR/ambiguous-config.json"
cat > "$ambiguous_config" <<'JSON'
{"version":1,"entries":[
  {"label":"Answer","text":"Answer selected","key":"TA"},
  {"label":"Brief","text":"Brief selected","key":"TB"}
]}
JSON
./target/debug/trievium --config "$ambiguous_config" >"$XDG_RUNTIME_DIR/ambiguous.log" 2>&1 &
picker_pid=$!
for _ in $(seq 1 100); do
  if swaymsg -s "$sway_sock" -t get_tree | grep -q '"app_id": "trievium"'; then
    break
  fi
  sleep 0.1
done
swaymsg -s "$sway_sock" '[app_id="trievium"] focus'
sleep 0.2
wtype t
sleep 0.25
if ! kill -0 "$picker_pid" 2>/dev/null; then
  echo "Ambiguous prefix prematurely closed the picker" >&2
  exit 1
fi
if [[ "$(wl-paste -n 2>/dev/null || true)" != "Unique prefix selected" ]]; then
  echo "Ambiguous prefix prematurely copied an entry" >&2
  exit 1
fi
wtype x
sleep 0.1
if [[ "$(wl-paste -n 2>/dev/null || true)" != "Unique prefix selected" ]]; then
  echo "Invalid letter changed clipboard or triggered a selection" >&2
  exit 1
fi
wtype b
copied=""
for _ in $(seq 1 70); do
  copied=$(wl-paste -n 2>/dev/null || true)
  if [[ "$copied" == "Brief selected" ]]; then
    break
  fi
  sleep 0.1
done
if [[ "$copied" != "Brief selected" ]]; then
  echo "Ambiguous prefix did not resolve after disambiguating letter" >&2
  cat "$XDG_RUNTIME_DIR/ambiguous.log" >&2
  exit 1
fi
wait_for_picker_exit
echo "Wayland prefix E2E passed: unique, ambiguous, and ignored invalid letters."
