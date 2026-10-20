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
export WLR_RENDERER=gles2
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
echo "Wayland E2E passed: G copied Hello!"
