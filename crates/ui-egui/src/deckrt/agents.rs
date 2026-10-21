//! deckrt: File › Options › AI Agents… (also the Help menu on a Mac and the command palette). Adds deckrt's MCP server
//! (`deckcraft-cli mcp --window 7979`) to Claude Desktop, or copies what Claude Code, Cursor and
//! other agents need, and lets agents work in this window: the window listens on 127.0.0.1:7979
//! while the box is ticked, and refuses every request while it isn't.

use std::path::PathBuf;

use deckcraft_engine::cmd::deckrt_agents::PORT;
use egui::{Color32, RichText, Ui};
use serde_json::{Value, json};

use crate::SlideApp;

pub const DIALOG: &str = "deckrt.agents";

/// The method the MCP server asks first (`deckcraft_mcp::PING`).
const PING: &str = "deckrt.ping";
/// How the window refuses while agents are kept out (`deckcraft_mcp::SHUT`).
const SHUT: &str = "deckrt isn't letting agents work in this window";

#[derive(Default)]
pub struct State {
    /// The window listens on [`PORT`] (started from this page).
    pub listening: bool,
    /// Agents may work in this window.
    pub allowed: bool,
    /// What the last button did, or what went wrong.
    pub note: Option<(String, bool)>,
}

/// The control channel's first look at a request: refused while agents are kept out, and the
/// MCP server's ping answered. `None` lets the request run.
pub fn gate(app: &SlideApp, method: &str) -> Option<Value> {
    let a = &app.deckrt.agents;
    if a.listening && !a.allowed {
        return Some(json!({"ok": false, "error": format!("{SHUT}. Turn on File › Options › AI Agents › Let agents work in this window.")}));
    }
    (method == PING).then(|| json!({"ok": true, "result": {"app": "deckrt", "version": env!("CARGO_PKG_VERSION")}}))
}

/// `deckcraft-cli` beside the running deckrt, which agents start.
pub fn cli_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let name = if cfg!(windows) { "deckcraft-cli.exe" } else { "deckcraft-cli" };
    let p = exe.parent()?.join(name);
    p.exists().then_some(p)
}

/// Claude Desktop's settings file. On Windows the Microsoft Store copy of Claude keeps its own,
/// which wins when it is there.
pub fn claude_desktop_config() -> Option<PathBuf> {
    const FILE: &str = "claude_desktop_config.json";
    if cfg!(windows) {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            let packages = PathBuf::from(local).join("Packages");
            if let Ok(rd) = std::fs::read_dir(&packages) {
                for e in rd.flatten() {
                    if e.file_name().to_string_lossy().starts_with("Claude_") {
                        let p = e.path().join("LocalCache").join("Roaming").join("Claude").join(FILE);
                        if p.exists() {
                            return Some(p);
                        }
                    }
                }
            }
        }
        return std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("Claude").join(FILE));
    }
    let home = PathBuf::from(std::env::var_os("HOME")?);
    if cfg!(target_os = "macos") {
        return Some(home.join("Library").join("Application Support").join("Claude").join(FILE));
    }
    let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".config"));
    Some(config.join("Claude").join(FILE))
}

/// Let agents work in this window, or keep them out.
pub fn set_allowed(app: &mut SlideApp, ctx: &egui::Context, on: bool) {
    if on && !app.deckrt.agents.listening {
        if app.control_rx.is_some() {
            // Started with --control: the window takes commands on that port already.
            app.deckrt.agents.allowed = true;
            return;
        }
        let Some(start) = app.services.start_control.as_mut() else {
            app.deckrt.agents.note = Some(("Agents can work in the window of the desktop app only.".into(), true));
            return;
        };
        match start(PORT, ctx.clone()) {
            Ok(rx) => {
                app.control_rx = Some(rx);
                app.deckrt.agents.listening = true;
            }
            Err(_) => {
                app.deckrt.agents.note =
                    Some((format!("Another program is using port {PORT}, so agents can't reach this window. Close it and try again."), true));
                return;
            }
        }
    }
    app.deckrt.agents.allowed = on;
}

fn section(ui: &mut Ui, title: &str) {
    ui.add_space(10.0);
    ui.label(RichText::new(title).strong().size(14.0));
    ui.add_space(2.0);
}

fn dim(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(12.5).color(ui.visuals().weak_text_color()));
}

/// The page. Returns whether it closes.
pub fn body(app: &mut SlideApp, ui: &mut Ui) -> bool {
    ui.set_min_width(560.0);
    ui.set_max_width(560.0);
    ui.label(
        "Claude Desktop, Claude Code, Cursor and other AI agents can make and edit decks in deckrt, polls, popups and the other live objects included. Add deckrt to the agent once, then ask it for a deck.",
    );
    let cli = cli_path();
    section(ui, "Add deckrt to an agent");
    match &cli {
        None => {
            dim(ui, "deckcraft-cli isn't in deckrt's folder, so agents can't be added from this copy of deckrt. Reinstall deckrt to get it back.")
        }
        Some(cli) => {
            let command = cli.to_string_lossy().to_string();
            let settings = app.session.execute("deckrt.agentSettings", &json!({"command": command})).unwrap_or_default();
            let json_text = settings["json"].as_str().unwrap_or_default().to_string();
            let claude_code = settings["claudeCode"].as_str().unwrap_or_default().to_string();
            let config = claude_desktop_config();
            ui.horizontal(|ui| {
                let add = ui.add_enabled(config.is_some(), egui::Button::new("Add to Claude Desktop"));
                if add.clicked()
                    && let Some(path) = &config
                {
                    let r = app.session.execute("deckrt.addToClaudeDesktop", &json!({"command": command, "configPath": path.to_string_lossy()}));
                    app.deckrt.agents.note = Some(match r {
                        Ok(_) => ("Added to Claude Desktop. Quit Claude Desktop and open it again to load deckrt.".into(), false),
                        Err(e) => (crate::deckrt::messages::plain_error(&e.to_string()), true),
                    });
                }
                if let Some(path) = &config {
                    add.on_hover_text(format!("Adds deckrt to {}. The file as it was is kept beside it.", path.display()));
                }
                if ui.button("Copy for Claude Code").on_hover_text(&claude_code).clicked() {
                    ui.ctx().copy_text(claude_code.clone());
                    app.deckrt.agents.note = Some(("Copied. Paste it in a terminal and press Enter.".into(), false));
                }
                if ui.button("Copy settings").on_hover_text("For Cursor and other agents: paste into their MCP settings").clicked() {
                    ui.ctx().copy_text(json_text.clone());
                    app.deckrt.agents.note = Some(("Copied. Paste it into the agent's MCP settings.".into(), false));
                }
            });
            if let Some((text, bad)) = &app.deckrt.agents.note {
                let c = if *bad { Color32::from_rgb(0xd8, 0x3f, 0x3f) } else { ui.visuals().text_color() };
                ui.label(RichText::new(text).color(c));
            }
            ui.add_space(4.0);
            egui::Frame::new().fill(ui.visuals().extreme_bg_color).inner_margin(8.0).corner_radius(4.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(&json_text).monospace().size(11.5));
            });
        }
    }
    section(ui, "Work in this window");
    let mut on = app.deckrt.agents.allowed;
    if ui.checkbox(&mut on, "Let agents work in this window").changed() {
        let ctx = ui.ctx().clone();
        set_allowed(app, &ctx, on);
    }
    dim(
        ui,
        "With this on, an agent's changes show up here as it makes them. Any program on this computer can then control deckrt, until you turn this off or close deckrt. With it off, agents build decks on their own and save them as files.",
    );
    ui.add_space(10.0);
    let mut done = false;
    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 26.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui.add(egui::Button::new("Close").min_size(egui::vec2(72.0, 24.0))).clicked() {
            done = true;
        }
    });
    done
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_keeping_agents_out_refuses_everything_and_one_letting_them_in_answers_the_ping() {
        let mut app = SlideApp::new(deckcraft_engine::Session::with_new(), crate::Services::default());
        // Not started from the page (no listener, or --control): nothing is gated.
        assert!(gate(&app, "engine.execute").is_none());
        assert_eq!(gate(&app, PING).unwrap()["ok"], true);
        app.deckrt.agents.listening = true;
        let refused = gate(&app, "engine.execute").unwrap();
        assert_eq!(refused["ok"], false);
        assert!(refused["error"].as_str().unwrap().starts_with(SHUT));
        assert_eq!(gate(&app, PING).unwrap()["ok"], false, "the ping too");
        app.deckrt.agents.allowed = true;
        assert!(gate(&app, "engine.execute").is_none());
        assert_eq!(gate(&app, PING).unwrap()["result"]["app"], "deckrt");
    }

    #[test]
    fn the_page_starts_the_listener_once_and_says_when_the_port_is_taken() {
        let started = std::rc::Rc::new(std::cell::Cell::new(0));
        let n = started.clone();
        let services = crate::Services {
            start_control: Some(Box::new(move |port, _| {
                assert_eq!(port, PORT);
                n.set(n.get() + 1);
                Ok(std::sync::mpsc::sync_channel(1).1)
            })),
            ..Default::default()
        };
        let mut app = SlideApp::new(deckcraft_engine::Session::with_new(), services);
        let ctx = egui::Context::default();
        set_allowed(&mut app, &ctx, true);
        set_allowed(&mut app, &ctx, false);
        set_allowed(&mut app, &ctx, true);
        assert_eq!(started.get(), 1);
        assert!(app.deckrt.agents.listening && app.deckrt.agents.allowed);

        let taken = crate::Services { start_control: Some(Box::new(|_, _| Err("in use".into()))), ..Default::default() };
        let mut app = SlideApp::new(deckcraft_engine::Session::with_new(), taken);
        set_allowed(&mut app, &ctx, true);
        assert!(!app.deckrt.agents.allowed);
        assert!(app.deckrt.agents.note.as_ref().is_some_and(|(t, bad)| *bad && t.contains("7979")));
    }
}
