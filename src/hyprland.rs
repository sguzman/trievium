//! One-shot Hyprland-specific floating-popup behavior.
//!
//! The compositor decides whether a Wayland toplevel is tiled and focused.
//! On Hyprland, identify our *mapped* window by both PID and exact app ID,
//! then send address-targeted commands. Never touch global window rules,
//! workspaces, unrelated clients, or running processes.
use serde_json::Value;
use std::process::Command;
use std::thread;
use std::time::Duration;

const APP_CLASS: &str = "trievium";
const RETRIES: usize = 40;
const RETRY_DELAY: Duration = Duration::from_millis(50);

pub fn focus_new_popup(width: i32, height: i32) {
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none_or(|s| s.is_empty()) {
        return;
    }
    let pid = std::process::id();
    thread::spawn(move || focus_when_mapped(pid, width, height));
}

fn focus_when_mapped(pid: u32, width: i32, height: i32) {
    for _ in 0..RETRIES {
        if let Some(address) = our_window_address(pid) {
            if let Err(error) = configure_our_window(&address, width, height) {
                eprintln!("trievium: could not configure Hyprland popup: {error}");
                return;
            }
            // A successful dispatcher response only means the command was
            // accepted. Verify that the compositor really focused this
            // window. Retry briefly during the initial mapping transition.
            if let Err(error) = confirm_popup_focus(&address) {
                eprintln!("trievium: {error}");
            }
            return;
        }
        thread::sleep(RETRY_DELAY);
    }
    eprintln!("trievium: timed out locating its mapped Hyprland window");
}

fn our_window_address(pid: u32) -> Option<String> {
    let response = Command::new("hyprctl")
        .args(["-j", "clients"])
        .output()
        .ok()?;
    if !response.status.success() {
        return None;
    }
    find_our_window(&response.stdout, pid)
}

fn find_our_window(bytes: &[u8], pid: u32) -> Option<String> {
    let clients: Vec<Value> = serde_json::from_slice(bytes).ok()?;
    clients.into_iter().find_map(|client| {
        let candidate_pid = client.get("pid")?.as_u64()?;
        let class = client.get("class")?.as_str()?;
        let mapped = client.get("mapped")?.as_bool()?;
        let address = client.get("address")?.as_str()?;
        if candidate_pid == u64::from(pid) && class == APP_CLASS && mapped && valid_address(address)
        {
            Some(address.to_owned())
        } else {
            None
        }
    })
}

fn valid_address(address: &str) -> bool {
    address
        .strip_prefix("0x")
        .is_some_and(|hex| !hex.is_empty() && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

/// Hyprland 0.55+ Lua configurations do not accept the old
/// `hyprctl dispatch setfloating address:...` syntax. The dispatch
/// argument must be ONE Lua dispatcher expression (hl.dsp.*).
///
/// Do not use an activewindow selector: every action targets Trievium's
/// verified window address, including centering and focus.
fn popup_dispatchers(address: &str, width: i32, height: i32) -> Result<[String; 4], String> {
    if !valid_address(address) {
        return Err("refusing invalid Hyprland window address".into());
    }
    if width <= 0 || height <= 0 {
        return Err("refusing nonpositive popup dimensions".into());
    }
    let selector = format!("address:{address}");
    Ok([
        format!("hl.dsp.window.float({{ window = \"{selector}\", action = \"enable\" }})"),
        format!("hl.dsp.window.resize({{ window = \"{selector}\", x = {width}, y = {height} }})"),
        format!("hl.dsp.window.center({{ window = \"{selector}\" }})"),
        format!("hl.dsp.focus({{ window = \"{selector}\" }})"),
    ])
}

fn dispatch_lua(expression: &str) -> Result<(), String> {
    let output = Command::new("hyprctl")
        .args(["dispatch", expression])
        .output()
        .map_err(|error| format!("starting hyprctl: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "Hyprland rejected {expression}: {}{}",
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn configure_our_window(address: &str, width: i32, height: i32) -> Result<(), String> {
    for expression in popup_dispatchers(address, width, height)? {
        dispatch_lua(&expression)?;
    }
    Ok(())
}

fn active_window_address(bytes: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    let address = value.get("address")?.as_str()?;
    valid_address(address).then(|| address.to_owned())
}

fn is_active_window(address: &str) -> bool {
    let Ok(output) = Command::new("hyprctl")
        .args(["-j", "activewindow"])
        .output()
    else {
        return false;
    };
    output.status.success() && active_window_address(&output.stdout).as_deref() == Some(address)
}

/// Compositor mapping has a settling period. Previously we returned
/// after the *first* observed focus success, even if focus reverted to the
/// launching terminal milliseconds later. Watch a short, bounded interval
/// after mapping, correcting only our own window's focus. This is NOT a
/// perpetual focus-stealing thread.
fn confirm_popup_focus(address: &str) -> Result<(), String> {
    if !valid_address(address) {
        return Err("invalid popup address".into());
    }
    let focus = format!("hl.dsp.focus({{ window = \"address:{address}\" }})");
    let mut observed_focus = false;
    for _ in 0..24 {
        if is_active_window(address) {
            observed_focus = true;
        } else {
            dispatch_lua(&focus)?;
        }
        thread::sleep(Duration::from_millis(125));
    }
    if is_active_window(address) {
        Ok(())
    } else if observed_focus {
        Err("Trievium window lost focus during startup".into())
    } else {
        Err("Hyprland never focused Trievium during startup".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_our_pid_exact_app_class_and_mapped_state() {
        let clients = br#"[
            {"pid":123,"class":"kitty","mapped":true,"address":"0x100"},
            {"pid":124,"class":"trievium","mapped":true,"address":"0x200"},
            {"pid":123,"class":"trievium","mapped":false,"address":"0x300"},
            {"pid":123,"class":"trievium","mapped":true,"address":"0x400"}
        ]"#;
        assert_eq!(find_our_window(clients, 123).as_deref(), Some("0x400"));
        assert_eq!(find_our_window(clients, 125), None);
    }

    #[test]
    fn rejects_invalid_window_addresses() {
        for address in ["", "0x", "0x123,activewindow", "123", "0x-1"] {
            assert!(!valid_address(address));
        }
        assert!(valid_address("0x1a2BF0"));
    }

    #[test]
    fn active_window_verification_requires_matching_address() {
        assert_eq!(
            active_window_address(br#"{"address":"0x1234","class":"trievium"}"#).as_deref(),
            Some("0x1234")
        );
        assert_eq!(active_window_address(br#"{"address":"garbage"}"#), None);
        assert_eq!(active_window_address(br#"{"pid":111}"#), None);
        assert_eq!(active_window_address(b"not json"), None);
    }

    #[test]
    fn malformed_hyprland_json_is_harmless() {
        assert_eq!(find_our_window(b"not json", 123), None);
        assert_eq!(find_our_window(br#"[{"pid":123}]"#, 123), None);
    }
    #[test]
    fn lua_dispatchers_are_explicit_and_target_only_our_window() {
        let statements = popup_dispatchers("0xabc123", 560, 420).unwrap();
        assert_eq!(
            statements,
            [
                "hl.dsp.window.float({ window = \"address:0xabc123\", action = \"enable\" })",
                "hl.dsp.window.resize({ window = \"address:0xabc123\", x = 560, y = 420 })",
                "hl.dsp.window.center({ window = \"address:0xabc123\" })",
                "hl.dsp.focus({ window = \"address:0xabc123\" })",
            ]
        );
        assert!(popup_dispatchers("address:activewindow", 560, 420).is_err());
        assert!(popup_dispatchers("0xabc123", 0, 420).is_err());
    }

    #[test]
    fn lua_expressions_execute_with_dispatcher_object_shapes() {
        // GitHub CI installs lua5.4. Outside CI, the test can run without Lua
        // and will skip only this additional syntax/argument verification.
        let interpreter = if Command::new("lua5.4").arg("-v").output().is_ok() {
            "lua5.4"
        } else {
            return;
        };
        let prelude = r#"
local function selected(t)
    assert(type(t) == "table")
    assert(t.window == "address:0xabc123")
    return t
end
hl = {
    dsp = {
        window = {
            float = function(t) assert(t.action == "enable"); return selected(t) end,
            resize = function(t) assert(t.x == 560 and t.y == 420); return selected(t) end,
            center = selected,
        },
        focus = selected,
    },
    dispatch = function(t) assert(type(t) == "table") end,
}
"#;
        for expression in popup_dispatchers("0xabc123", 560, 420).unwrap() {
            let script = format!("{prelude}\nhl.dispatch({expression})");
            let output = Command::new(interpreter)
                .args(["-e", &script])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "Lua rejected generated dispatcher {expression}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}
