//! deckrt: AI Agents. The settings an agent (Claude Desktop, Claude Code, Cursor…) needs to run
//! deckrt's MCP server, and Add to Claude Desktop, which puts them in Claude Desktop's settings
//! file. Only the `deckrt` entry under `mcpServers` changes; everything else in the file stays,
//! and the file as it was is kept beside it the first time.

use serde_json::{Map, Value, json};

use super::{CommandSpec, always, bad, cmd};
use crate::{Result, Session};

/// The name agents list the server under.
pub const SERVER_NAME: &str = "deckrt";
/// The port the deckrt window listens on for agents (`deckcraft-cli mcp --window 7979`).
pub const PORT: u16 = 7979;
/// Added to the settings file's name for the copy kept the first time.
pub const BACKUP_SUFFIX: &str = ".before-deckrt";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "deckrt.agentSettings",
            "AI Agent Settings",
            [],
            None,
            "{command: path of deckcraft-cli} → {json: the mcpServers settings, claudeCode: the `claude mcp add` line, args}",
            always,
            settings
        ),
        cmd!(
            noundo "deckrt.addToClaudeDesktop",
            "Add to Claude Desktop",
            [],
            None,
            "{command: path of deckcraft-cli, configPath: claude_desktop_config.json} → {path, backup?, replaced}",
            always,
            add_to_claude_desktop
        ),
    ]
}

/// The arguments the server runs with: in the deckrt window when it lets agents in, else
/// headless.
pub fn server_args() -> Vec<String> {
    vec!["mcp".into(), "--window".into(), PORT.to_string()]
}

/// The `mcpServers` entry for `command`.
pub fn server_entry(command: &str) -> Value {
    json!({"command": command, "args": server_args()})
}

/// `claude mcp add` for Claude Code, quoted for a path with spaces.
pub fn claude_code_line(command: &str) -> String {
    format!("claude mcp add {SERVER_NAME} -- \"{command}\" {}", server_args().join(" "))
}

fn command_param(p: &Value, cmd: &str) -> Result<String> {
    p.get("command").and_then(Value::as_str).filter(|c| !c.trim().is_empty()).map(str::to_string).ok_or_else(|| bad(cmd, "missing `command`"))
}

fn settings(_s: &mut Session, p: &Value) -> Result<Value> {
    let command = command_param(p, "deckrt.agentSettings")?;
    let json = json!({"mcpServers": {SERVER_NAME: server_entry(&command)}});
    Ok(json!({
        "json": serde_json::to_string_pretty(&json).unwrap_or_default(),
        "claudeCode": claude_code_line(&command),
        "args": server_args(),
    }))
}

fn add_to_claude_desktop(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "deckrt.addToClaudeDesktop";
    let command = command_param(p, cmd)?;
    let path = p.get("configPath").and_then(Value::as_str).filter(|c| !c.trim().is_empty()).ok_or_else(|| bad(cmd, "missing `configPath`"))?;
    let existing =
        if crate::io::exists(path) { Some(crate::io::read_file(path).map_err(|e| bad(cmd, format!("couldn't read {path}: {e}")))?) } else { None };
    let mut root = match existing.as_deref() {
        None => Map::new(),
        Some(b) if String::from_utf8_lossy(b).trim().is_empty() => Map::new(),
        Some(b) => match serde_json::from_slice::<Value>(b) {
            Ok(Value::Object(m)) => m,
            _ => return Err(bad(cmd, format!("{path} isn't a settings file deckrt can read, so it was left as it is"))),
        },
    };
    let servers = root.entry("mcpServers").or_insert_with(|| json!({}));
    if !servers.is_object() {
        return Err(bad(cmd, format!("`mcpServers` in {path} isn't a list deckrt can add to, so the file was left as it is")));
    }
    let replaced = servers.get(SERVER_NAME).is_some();
    servers[SERVER_NAME] = server_entry(&command);
    // The file as it was, kept once: a second Add doesn't replace the person's own copy.
    let mut backup = None;
    if let Some(b) = &existing {
        let to = format!("{path}{BACKUP_SUFFIX}");
        if !crate::io::exists(&to) {
            crate::io::write_file(&to, b).map_err(|e| bad(cmd, format!("couldn't keep a copy of {path}: {e}")))?;
        }
        backup = Some(to);
    }
    if let Some(dir) = std::path::Path::new(path).parent().filter(|d| !d.as_os_str().is_empty()) {
        crate::io::make_dir(&dir.to_string_lossy()).map_err(|e| bad(cmd, format!("couldn't make {}: {e}", dir.display())))?;
    }
    let text = serde_json::to_string_pretty(&Value::Object(root)).unwrap_or_default();
    crate::io::write_file(path, text.as_bytes()).map_err(|e| bad(cmd, format!("couldn't write {path}: {e}")))?;
    Ok(json!({"path": path, "backup": backup, "replaced": replaced}))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("deckrt-agents-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_settings_name_the_server_and_its_window_port() {
        let mut s = Session::new();
        let r = s.execute("deckrt.agentSettings", &json!({"command": "C:\\Program Files\\deckrt\\deckcraft-cli.exe"})).unwrap();
        let v: Value = serde_json::from_str(r["json"].as_str().unwrap()).unwrap();
        assert_eq!(v["mcpServers"]["deckrt"]["command"], "C:\\Program Files\\deckrt\\deckcraft-cli.exe");
        assert_eq!(v["mcpServers"]["deckrt"]["args"], json!(["mcp", "--window", "7979"]));
        assert_eq!(r["claudeCode"], "claude mcp add deckrt -- \"C:\\Program Files\\deckrt\\deckcraft-cli.exe\" mcp --window 7979");
    }

    #[test]
    fn adding_keeps_the_other_servers_and_a_copy_of_the_file() {
        let d = scratch("merge");
        let path = d.join("Claude").join("claude_desktop_config.json");
        let p = |c: &str| json!({"command": c, "configPath": path.to_string_lossy()});
        let mut s = Session::new();
        // No file yet: made, folder and all, with no copy to keep.
        let r = s.execute("deckrt.addToClaudeDesktop", &p("/a/deckcraft-cli")).unwrap();
        assert_eq!(r["backup"], Value::Null);
        assert_eq!(r["replaced"], false);
        // The person's own settings: other servers and keys stay, deckrt is replaced.
        let own = json!({"mcpServers": {"files": {"command": "npx", "args": ["fs"]}, "deckrt": {"command": "old"}}, "globalShortcut": "Ctrl+Space"});
        std::fs::write(&path, serde_json::to_string(&own).unwrap()).unwrap();
        let r = s.execute("deckrt.addToClaudeDesktop", &p("/b/deckcraft-cli")).unwrap();
        assert_eq!(r["replaced"], true);
        let now: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(now["mcpServers"]["files"], own["mcpServers"]["files"]);
        assert_eq!(now["globalShortcut"], "Ctrl+Space");
        assert_eq!(now["mcpServers"]["deckrt"]["command"], "/b/deckcraft-cli");
        let backup = std::path::PathBuf::from(r["backup"].as_str().unwrap());
        let kept: Value = serde_json::from_slice(&std::fs::read(&backup).unwrap()).unwrap();
        assert_eq!(kept, own);
        // Again: the first copy stays the person's own file.
        s.execute("deckrt.addToClaudeDesktop", &p("/c/deckcraft-cli")).unwrap();
        let kept: Value = serde_json::from_slice(&std::fs::read(&backup).unwrap()).unwrap();
        assert_eq!(kept, own);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_file_deckrt_cannot_read_is_left_alone() {
        let d = scratch("broken");
        let path = d.join("claude_desktop_config.json");
        std::fs::write(&path, b"{ not json").unwrap();
        let mut s = Session::new();
        let e = s.execute("deckrt.addToClaudeDesktop", &json!({"command": "x", "configPath": path.to_string_lossy()})).unwrap_err();
        assert!(e.to_string().contains("left as it is"), "{e}");
        assert_eq!(std::fs::read(&path).unwrap(), b"{ not json");
        assert!(!d.join(format!("claude_desktop_config.json{BACKUP_SUFFIX}")).exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
