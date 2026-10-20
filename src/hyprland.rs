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
        if candidate_pid == u64::from(pid)
            && class == APP_CLASS
            && mapped
            && valid_address(address)
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

fn dispatch(action: &str, selector: &str) -> Result<(), String> {
    let status = Command::new("hyprctl")
        .args(["dispatch", action, selector])
        .status()
        .map_err(|err| format!("hyprctl dispatch {action}: {err}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("hyprctl dispatch {action} returned {status}"))
    }
}

fn configure_our_window(address: &str, width: i32, height: i32) -> Result<(), String> {
    if !valid_address(address) {
        return Err("refusing invalid Hyprland window address".into());
    }
    let selector = format!("address:{address}");
    dispatch("setfloating", &selector)?;
    dispatch(
        "resizewindowpixel",
        &format!("exact {width} {height},{selector}"),
    )?;
    dispatch("focuswindow", &selector)?;

    // The centering dispatcher addresses the *focused* window, not a
    // selector. Read it back before centering so an unsuccessful focus
    // command cannot reposition the terminal or any unrelated window.
    if is_active_window(address) {
        dispatch("centerwindow", "1")?;
    }
    Ok(())
}

fn is_active_window(address: &str) -> bool {
    let Ok(output) = Command::new("hyprctl")
        .args(["-j", "activewindow"])
        .output()
    else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    serde_json::from_slice::<Value>(&output.stdout)
        .ok()
        .and_then(|value| value.get("address")?.as_str().map(str::to_owned))
        .is_some_and(|active| active == address)
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
    fn malformed_hyprland_json_is_harmless() {
        assert_eq!(find_our_window(b"not json", 123), None);
        assert_eq!(find_our_window(br#"[{"pid":123}]"#, 123), None);
    }
}
