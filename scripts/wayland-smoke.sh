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


# Command-tree regression: a single remaining candidate must NOT execute at
# the first key. Only the complete configured sequence launches the program.
# This must not read, mutate, or replace the copy library or clipboard.
command_config="$XDG_RUNTIME_DIR/commands.json"
command_marker="$XDG_RUNTIME_DIR/command-completed"
deep_marker="$XDG_RUNTIME_DIR/deep-command-completed"
cp "$config" "$XDG_RUNTIME_DIR/copy-before-commands.json"
cat > "$command_config" <<JSON
{"version":1,"groups":[
  {"prefix":"w","label":"Window commands"},
  {"prefix":"w a","label":"Advanced commands"}
],"commands":[
  {"sequence":"w f","label":"Complete marker",
   "action":{"type":"exec","program":"touch","args":["$command_marker"]}},
  {"sequence":"w a t","label":"Deep marker",
   "action":{"type":"exec","program":"touch","args":["$deep_marker"]}}
]}
JSON
./target/debug/trievium commands validate --config "$command_config"
./target/debug/trievium commands --config "$command_config" >"$XDG_RUNTIME_DIR/commands.log" 2>&1 &
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
  echo "Command HUD did not map on Wayland" >&2
  cat "$XDG_RUNTIME_DIR/commands.log" >&2
  exit 1
fi
swaymsg -s "$sway_sock" '[app_id="trievium"] focus'
sleep 0.2
wtype w
sleep 0.3
if [[ -e "$command_marker" || -e "$deep_marker" ]] || ! kill -0 "$picker_pid" 2>/dev/null; then
  echo "Command HUD executed partial prefix prematurely" >&2
  cat "$XDG_RUNTIME_DIR/commands.log" >&2
  exit 1
fi
wtype x
sleep 0.15
if [[ -e "$command_marker" || -e "$deep_marker" ]]; then
  echo "Invalid command key triggered an action" >&2
  exit 1
fi
wtype -k BackSpace
wtype w
wtype a
sleep 0.15
if [[ -e "$deep_marker" ]] || ! kill -0 "$picker_pid" 2>/dev/null; then
  echo "Nested command prefix executed prematurely" >&2
  exit 1
fi
wtype t
for _ in $(seq 1 70); do
  if [[ -e "$deep_marker" ]]; then
    break
  fi
  sleep 0.1
done
if [[ ! -e "$deep_marker" ]]; then
  echo "Backspace or three-key command traversal failed" >&2
  cat "$XDG_RUNTIME_DIR/commands.log" >&2
  exit 1
fi
wait_for_picker_exit

# Reopen and verify a different command shares the first prefix.
./target/debug/trievium commands --config "$command_config" >"$XDG_RUNTIME_DIR/commands-second.log" 2>&1 &
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
  echo "Second command HUD did not map" >&2
  cat "$XDG_RUNTIME_DIR/commands-second.log" >&2
  exit 1
fi
swaymsg -s "$sway_sock" '[app_id="trievium"] focus'
sleep 0.2
wtype w
sleep 0.15
wtype f
for _ in $(seq 1 70); do
  if [[ -e "$command_marker" ]]; then
    break
  fi
  sleep 0.1
done
if [[ ! -e "$command_marker" ]]; then
  echo "Command HUD did not execute the completed sequence" >&2
  cat "$XDG_RUNTIME_DIR/commands.log" >&2
  exit 1
fi
wait_for_picker_exit
cmp "$config" "$XDG_RUNTIME_DIR/copy-before-commands.json"
if [[ "$(wl-paste -n 2>/dev/null || true)" != "Brief selected" ]]; then
  echo "Command execution unexpectedly changed the clipboard" >&2
  exit 1
fi
echo "Wayland commands E2E passed: exact sequence executed; clipboard and items preserved."


# Native command editor is independently launchable and Escape must not
# modify the command registry (or the copy library).
cp "$command_config" "$XDG_RUNTIME_DIR/commands-before-editor.json"
./target/debug/trievium commands edit --config "$command_config" >"$XDG_RUNTIME_DIR/command-editor.log" 2>&1 &
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
  echo "Command editor did not map on Wayland" >&2
  cat "$XDG_RUNTIME_DIR/command-editor.log" >&2
  exit 1
fi
swaymsg -s "$sway_sock" '[app_id="trievium"] focus'
sleep 0.2
wtype -k Escape
wait_for_picker_exit
cmp "$command_config" "$XDG_RUNTIME_DIR/commands-before-editor.json"
cmp "$config" "$XDG_RUNTIME_DIR/copy-before-commands.json"
echo "Wayland command editor E2E passed: native editor opened and closed without modifying either library."


# On a fresh installation, opening the command editor first must initialize
# only its own registry; no separate 'commands init' prerequisite is required.
fresh_commands="$XDG_RUNTIME_DIR/first-run-commands.json"
test ! -e "$fresh_commands"
./target/debug/trievium commands edit --config "$fresh_commands" >"$XDG_RUNTIME_DIR/command-first-run.log" 2>&1 &
picker_pid=$!
found=""
for _ in $(seq 1 100); do
  found=$(swaymsg -s "$sway_sock" -t get_tree | grep -o '"app_id": "trievium"' | head -1 || true)
  if [[ -n "$found" ]]; then
    break
  fi
  sleep 0.1
done
if [[ -z "$found" || ! -f "$fresh_commands" ]]; then
  echo "First-run command editor did not create and open its own registry" >&2
  cat "$XDG_RUNTIME_DIR/command-first-run.log" >&2
  exit 1
fi
# A second command HUD during first-run editing must exit as Busy,
# not start another native window or rewrite the registry.
timeout 5s ./target/debug/trievium commands --config "$fresh_commands" >"$XDG_RUNTIME_DIR/command-concurrent.log" 2>&1
if ! grep -Fq 'another HUD is already open' "$XDG_RUNTIME_DIR/command-concurrent.log"; then
  echo "Second command invocation ignored the single-instance lock" >&2
  cat "$XDG_RUNTIME_DIR/command-concurrent.log" >&2
  exit 1
fi
swaymsg -s "$sway_sock" '[app_id="trievium"] focus'
sleep 0.2
wtype -k Escape
wait_for_picker_exit
./target/debug/trievium commands validate --config "$fresh_commands"
cmp "$config" "$XDG_RUNTIME_DIR/copy-before-commands.json"
echo "Wayland fresh-editor E2E passed: independent registry initialized with no copy-library mutation."


# Native editor Done is a real pointer interaction, not merely an Escape
# smoke test. In the isolated 1280x720 nested Sway compositor, click the
# editor's Done control, then verify the picker resumed and can execute the
# configured exact sequence. This does not interact with the host desktop.
rm -f "$command_marker"
./target/debug/trievium commands edit --config "$command_config" >"$XDG_RUNTIME_DIR/command-editor-done.log" 2>&1 &
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
  echo "Command editor for pointer round trip did not map" >&2
  cat "$XDG_RUNTIME_DIR/command-editor-done.log" >&2
  exit 1
fi
swaymsg -s "$sway_sock" '[app_id="trievium"] focus' > /dev/null
sleep 0.3
# Fixed-layout editor header in this dedicated output: Save then Done.
swaymsg -s "$sway_sock" 'seat seat0 cursor set 250 15' > /dev/null
# Use the compositor's virtual-pointer protocol rather than Sway's
# deprecated seat press/release injection (which may not deliver a Wayland
# pointer event when the only physical device is a virtual keyboard).
wlrctl pointer move 1 1
wlrctl pointer click left
sleep 0.5
wtype w
wtype f
for _ in $(seq 1 70); do
  if [[ -e "$command_marker" ]]; then
    break
  fi
  sleep 0.1
done
if [[ ! -e "$command_marker" ]]; then
  echo "Pointer Done did not return the command editor to an executable HUD" >&2
  cat "$XDG_RUNTIME_DIR/command-editor-done.log" >&2
  exit 1
fi
wait_for_picker_exit
cmp "$config" "$XDG_RUNTIME_DIR/copy-before-commands.json"
echo "Wayland command editor Done E2E passed: pointer clicked Done, resumed HUD and executed a command."
