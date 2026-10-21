//! deckrt: the backend an AI agent gets by default. The first call decides where the work
//! happens: in the open deckrt window when it lets agents in (AI Agents › Let agents work in this
//! window), otherwise headless, as a deck saved to a file. `use_window` moves a deck built
//! headless into the window later.

use serde_json::{Value, json};

use crate::backend::{Backend, Remote};
use crate::headless::Headless;

/// The method the window answers when it lets agents in. A window that doesn't answers every
/// method with an error starting with [`SHUT`].
pub const PING: &str = "deckrt.ping";
/// How a window with agents turned off refuses (the app's control handling uses it).
pub const SHUT: &str = "deckrt isn't letting agents work in this window";

enum Where {
    Undecided,
    App(Remote),
    Alone(Box<Headless>),
}

pub struct Window {
    addr: String,
    at: Where,
}

impl Window {
    pub fn new(addr: &str) -> Self {
        Window { addr: addr.to_string(), at: Where::Undecided }
    }

    /// The open window, when it lets agents in.
    fn app(&self) -> Result<Remote, String> {
        let mut r = Remote::connect(&self.addr).map_err(|_| "deckrt isn't open, or AI Agents › Let agents work in this window is off".to_string())?;
        match r.call(PING, json!({})) {
            Ok(_) => Ok(r),
            Err(e) if e.starts_with(SHUT) => Err(format!("{e}: turn on AI Agents › Let agents work in this window in deckrt")),
            Err(e) => Err(e),
        }
    }

    /// Tests: as if the first call found no window.
    #[cfg(test)]
    pub fn at_headless_for_test(&mut self) {
        self.at = Where::Alone(Box::default());
    }

    fn decide(&mut self) {
        if matches!(self.at, Where::Undecided) {
            self.at = match self.app() {
                Ok(r) => Where::App(r),
                Err(_) => Where::Alone(Box::default()),
            };
        }
    }
}

impl Backend for Window {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.decide();
        match &mut self.at {
            Where::App(r) => r.call(method, params),
            Where::Alone(h) => h.call(method, params),
            Where::Undecided => Err("no backend".into()),
        }
    }

    fn has_ui(&self) -> bool {
        matches!(self.at, Where::App(_))
    }

    fn describe(&self) -> String {
        match &self.at {
            Where::Undecided => format!(
                "the deckrt window at {} when deckrt is open and lets agents in (AI Agents › Let agents work in this window), otherwise headless: decks are built in memory and saved with save_presentation, and open_in_deckrt moves one into the window",
                self.addr
            ),
            Where::App(r) => r.describe(),
            Where::Alone(_) => {
                "headless (no deckrt window): save_presentation writes the deck; open_in_deckrt moves it into the window once deckrt lets agents in"
                    .into()
            }
        }
    }

    fn use_window(&mut self) -> Result<Value, String> {
        if matches!(self.at, Where::App(_)) {
            return Ok(json!({"window": true, "moved": false, "text": "Already working in the deckrt window."}));
        }
        let mut app = self.app()?;
        // A deck built headless goes across whole (the deckrt format keeps every object).
        let moved = match &mut self.at {
            Where::Alone(h) if h.session.active().is_some() => {
                let title = h.session.active().map(|d| d.title()).unwrap_or_else(|| "Presentation".into());
                let data = h.call("engine.execute", json!({"command": "file.saveBytes", "params": {"format": "deckcraft"}}))?;
                let data = data.get("data").cloned().ok_or("the deck couldn't be saved to move it")?;
                app.call("engine.execute", json!({"command": "file.openBytes", "params": {"name": format!("{title}.deckcraft"), "data": data}}))?;
                Some(title)
            }
            _ => None,
        };
        self.at = Where::App(app);
        Ok(json!({
            "window": true,
            "moved": moved.is_some(),
            "text": match moved {
                Some(t) => format!("{t} is open in the deckrt window. The next calls work there."),
                None => "Working in the deckrt window now.".into(),
            }
        }))
    }
}
