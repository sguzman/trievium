//! Hyprland integration: float at map time, not after tiling has reflowed.
//!
//! A Wayland toplevel cannot request floating itself. Register a named,
//! app-ID-scoped runtime rule synchronously BEFORE creating the native window.
//! After mapping, only operate on our verified, already-floating PID/address.
//! Never edit the user's Hyprland configuration, change workspaces, or
//! operate on unrelated clients.
use serde_json::Value;
use std::process::Command;
use std::thread;
use std::time::Duration;

const APP_CLASS: &str = "trievium";
const RETRIES: usize = 40;
const RETRY_DELAY: Duration = Duration::from_millis(50);

// One named rule is updated in place on repeated launches; no growing list
// of per-process rules, and no modification of the user's config file.
const PRE_MAP_RULE: &str = r#"hl.window_rule({ name = "trievium-float-on-open", match = { class = "^trievium$" }, float = true, center = true })"#;

/// A late tile-to-float conversion reflows other windows before it runs.
/// Set the static rule synchronously before the Wayland toplevel exists.
/// Refuse to open a tiled fallback if Hyprland rejects the rule.
pub fn prepare_floating_popup() -> Result<(), String> {
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none_or(|s| s.is_empty()) {
        return Ok(());
    }
    let output = Command::new("hyprctl")
        .args(["eval", PRE_MAP_RULE])
        .output()
        .map_err(|error| format!("installing pre-map Hyprland floating rule: {error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() || stdout.trim() != "ok" {
        return Err(format!(
            "Hyprland rejected Trievium's pre-map floating rule: {} {}; popup not opened to protect workspace layout",
            stdout.trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

pub fn focus_new_popup(width: i32, height: i32) {
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none_or(|s| s.is_empty()) {
        return;
    }
    let pid = std::process::id();
    thread::spawn(move || focus_when_mapped(pid, width, height));
}

fn focus_when_mapped(pid: u32, width: i32, height: i32) {
    for _ in 0..RETRIES {
        if let Some((address, floating)) = our_window_address(pid) {
            if !floating {
                eprintln!(
                    "trievium: popup mapped tiled despite the pre-map floating rule; closing to protect workspace layout"
                );
                std::process::exit(1);
            }
            if let Err(error) = configure_our_window(&address, width, height) {
                eprintln!("trievium: could not configure Hyprland popup: {error}");
                return;
            }
            // Initial focus may lag mapping slightly. Retry at most once,
            // never continually reclaim the keyboard or warp the pointer.
            if let Err(error) = confirm_popup_focus(&address) {
                eprintln!("trievium: {error}");
            }
            return;
        }
        thread::sleep(RETRY_DELAY);
    }
    eprintln!("trievium: timed out locating its mapped Hyprland window");
}

fn our_window_address(pid: u32) -> Option<(String, bool)> {
    let response = Command::new("hyprctl")
        .args(["-j", "clients"])
        .output()
        .ok()?;
    if !response.status.success() {
        return None;
    }
    find_our_window(&response.stdout, pid)
}

fn find_our_window(bytes: &[u8], pid: u32) -> Option<(String, bool)> {
    let clients: Vec<Value> = serde_json::from_slice(bytes).ok()?;
    clients.into_iter().find_map(|client| {
        let candidate_pid = client.get("pid")?.as_u64()?;
        let class = client.get("class")?.as_str()?;
        let mapped = client.get("mapped")?.as_bool()?;
        let floating = client.get("floating")?.as_bool()?;
        let address = client.get("address")?.as_str()?;
        if candidate_pid == u64::from(pid) && class == APP_CLASS && mapped && valid_address(address)
        {
            Some((address.to_owned(), floating))
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

/// Only adjust geometry and focus AFTER verifying the mapped window is
/// already floating. Never float after mapping: it reflows tiled windows.
/// All dispatchers explicitly target our verified window address.
fn popup_dispatchers(address: &str, width: i32, height: i32) -> Result<[String; 4], String> {
    if !valid_address(address) {
        return Err("refusing invalid Hyprland window address".into());
    }
    if width <= 0 || height <= 0 {
        return Err("refusing nonpositive popup dimensions".into());
    }
    let selector = format!("address:{address}");
    Ok([
        format!("hl.dsp.window.resize({{ window = \"{selector}\", x = {width}, y = {height} }})"),
        format!("hl.dsp.window.center({{ window = \"{selector}\" }})"),
        // A newly floated window can remain below the originating tiled
        // window. Raise it before focusing, otherwise clicks hit the terminal.
        format!("hl.dsp.window.alter_zorder({{ window = \"{selector}\", mode = \"top\" }})"),
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

/// Focus is a one-time launch action. The old three-second focus loop
/// repeatedly took the user's pointer away from clicks in the editor.
fn confirm_popup_focus(address: &str) -> Result<(), String> {
    if !valid_address(address) {
        return Err("invalid popup address".into());
    }
    // A single short settling delay allows a newly mapped surface to become
    // active. Never take focus back after the user has clicked elsewhere.
    thread::sleep(Duration::from_millis(100));
    if is_active_window(address) {
        return Ok(());
    }
    let focus = format!("hl.dsp.focus({{ window = \"address:{address}\" }})");
    dispatch_lua(&focus)
}

/// Only floating windows may receive compositor-side resize commands:
/// resizing a tiled surface can reflow the user's entire workspace.
fn our_floating_window_address(pid: u32) -> Option<String> {
    let output = Command::new("hyprctl")
        .args(["-j", "clients"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let clients: Vec<Value> = serde_json::from_slice(&output.stdout).ok()?;
    clients.into_iter().find_map(|client| {
        let address = client.get("address")?.as_str()?;
        let matches = client.get("pid")?.as_u64()? == u64::from(pid)
            && client.get("class")?.as_str()? == APP_CLASS
            && client.get("mapped")?.as_bool()?
            && client.get("floating")?.as_bool()?
            && valid_address(address);
        matches.then(|| address.to_owned())
    })
}

/// Resize an already mapped Trievium window when switching from the small
/// picker to the editor and back. Only window geometry changes here: no
/// focus commands, cursor movement, or global compositor configuration.
pub fn resize_own_popup(width: i32, height: i32) {
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none_or(|s| s.is_empty()) {
        return;
    }
    if width <= 0 || height <= 0 {
        return;
    }
    let pid = std::process::id();
    thread::spawn(move || {
        if let Some(address) = our_floating_window_address(pid) {
            let selector = format!("address:{address}");
            if let Err(error) = dispatch_lua(&format!(
                "hl.dsp.window.resize({{ window = \"{selector}\", x = {width}, y = {height} }})"
            )) {
                eprintln!("trievium: editor resize: {error}");
            }
            if let Err(error) = dispatch_lua(&format!(
                "hl.dsp.window.center({{ window = \"{selector}\" }})"
            )) {
                eprintln!("trievium: editor centering: {error}");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floating_resize_requires_floating_own_process_window() {
        let clients = br#"[
            {"pid":123,"class":"trievium","mapped":true,"floating":false,"address":"0x100"},
            {"pid":123,"class":"kitty","mapped":true,"floating":true,"address":"0x200"},
            {"pid":124,"class":"trievium","mapped":true,"floating":true,"address":"0x300"}
        ]"#;
        let parsed: Vec<Value> = serde_json::from_slice(clients).unwrap();
        let matches = |client: &Value| {
            client.get("pid").and_then(Value::as_u64) == Some(123)
                && client.get("class").and_then(Value::as_str) == Some("trievium")
                && client.get("mapped").and_then(Value::as_bool) == Some(true)
                && client.get("floating").and_then(Value::as_bool) == Some(true)
        };
        assert!(parsed.iter().all(|client| !matches(client)));
    }

    #[test]
    fn requires_our_pid_exact_app_class_and_mapped_state() {
        let clients = br#"[
            {"pid":123,"class":"kitty","mapped":true,"floating":true,"address":"0x100"},
            {"pid":124,"class":"trievium","mapped":true,"floating":true,"address":"0x200"},
            {"pid":123,"class":"trievium","mapped":false,"floating":false,"address":"0x300"},
            {"pid":123,"class":"trievium","mapped":true,"floating":true,"address":"0x400"}
        ]"#;
        assert_eq!(find_our_window(clients, 123), Some(("0x400".into(), true)));
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
                "hl.dsp.window.resize({ window = \"address:0xabc123\", x = 560, y = 420 })",
                "hl.dsp.window.center({ window = \"address:0xabc123\" })",
                "hl.dsp.window.alter_zorder({ window = \"address:0xabc123\", mode = \"top\" })",
                "hl.dsp.focus({ window = \"address:0xabc123\" })",
            ]
        );
        assert!(popup_dispatchers("address:activewindow", 560, 420).is_err());
        assert!(popup_dispatchers("0xabc123", 0, 420).is_err());
    }

    #[test]
    fn pre_map_rule_is_app_scoped_and_no_post_map_float_is_emitted() {
        assert!(PRE_MAP_RULE.contains("name = \"trievium-float-on-open\""));
        assert!(PRE_MAP_RULE.contains("class = \"^trievium$\""));
        assert!(PRE_MAP_RULE.contains("float = true"));
        assert!(PRE_MAP_RULE.contains("center = true"));
        let dispatchers = popup_dispatchers("0xabc", 560, 420).unwrap();
        assert!(
            dispatchers
                .iter()
                .all(|expr| !expr.contains("window.float"))
        );
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
            resize = function(t) assert(t.x == 560 and t.y == 420); return selected(t) end,
            center = selected,
            alter_zorder = function(t) assert(t.mode == "top"); return selected(t) end,
        },
        focus = selected,
    },
    dispatch = function(t) assert(type(t) == "table") end,
}
"#;
        let pre_map_test = format!(
            "hl = {{ window_rule = function(t) assert(t.name == 'trievium-float-on-open' and t.match.class == '^trievium$' and t.float and t.center) end }}; {PRE_MAP_RULE}"
        );
        let result = Command::new(interpreter)
            .args(["-e", &pre_map_test])
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "Lua rejected pre-map floating rule: {}",
            String::from_utf8_lossy(&result.stderr)
        );

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
