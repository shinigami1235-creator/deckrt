//! deckrt: a start that survives a graphics driver crash.
//!
//! On some Windows machines the graphics driver crashes inside wgpu's start-up (an Intel Vulkan
//! driver during adapter enumeration, an AMD OpenGL driver) and the process dies before any Rust
//! error handling runs. A laptop with an Intel and an NVIDIA GPU has two drivers to go wrong.
//!
//! So on Windows `deckrt.exe` first starts itself again as a child, one graphics backend at a
//! time: DirectX 12, then Vulkan, then OpenGL, then DirectX 12 on the software adapter (WARP).
//! The child writes a marker file once its first frame is drawn. A child that dies before the
//! marker exists is a start-up failure and the next backend is tried; a child that dies after
//! it crashed for some other reason and is not restarted. The backend that worked is remembered
//! in the settings folder and tried first next time.
//!
//! `--no-supervise`, a `DECKRT_CHILD` environment variable, or an explicit `WGPU_BACKEND` turn
//! this off. The whole thing is only on by default on Windows; `DECKRT_SUPERVISE=1` turns it on
//! elsewhere, for tests.

use std::path::{Path, PathBuf};
use std::process::Command;

/// One way to start the graphics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attempt {
    /// The `WGPU_BACKEND` value.
    pub backend: &'static str,
    /// Pick the software adapter (`DECKRT_SOFTWARE_GPU=1`).
    pub software: bool,
}

impl Attempt {
    pub fn name(&self) -> String {
        if self.software { format!("{} (software)", self.backend) } else { self.backend.to_string() }
    }
}

pub const ATTEMPTS: [Attempt; 4] = [
    Attempt { backend: "dx12", software: false },
    Attempt { backend: "vulkan", software: false },
    Attempt { backend: "gl", software: false },
    Attempt { backend: "dx12", software: true },
];

/// The marker the child writes after its first frame.
pub const READY_ENV: &str = "DECKRT_READY_FILE";
/// Set in the child so it does not supervise in turn.
pub const CHILD_ENV: &str = "DECKRT_CHILD";
/// Set in the child for the software-adapter attempt (read by `graphics::configure`).
pub const SOFTWARE_ENV: &str = "DECKRT_SOFTWARE_GPU";

/// Whether this process should supervise a child instead of opening the window itself.
pub fn wanted(args: &[String], env: impl Fn(&str) -> Option<String>) -> bool {
    if args.iter().any(|a| a == "--no-supervise" || a == "--help" || a == "-h" || a == "--version" || a == "-V") {
        return false;
    }
    if env(CHILD_ENV).is_some() || env("WGPU_BACKEND").is_some() {
        return false;
    }
    cfg!(target_os = "windows") || env("DECKRT_SUPERVISE").is_some_and(|v| v == "1")
}

/// The attempts in order, the remembered one first.
pub fn order(remembered: Option<&str>) -> Vec<Attempt> {
    let mut out: Vec<Attempt> = ATTEMPTS.to_vec();
    if let Some(r) = remembered
        && let Some(i) = out.iter().position(|a| a.name() == r)
    {
        let first = out.remove(i);
        out.insert(0, first);
    }
    out
}

/// What happened to one child.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The window came up; the exit code is the app's own.
    Ran(i32),
    /// Died before the first frame.
    FailedToStart(String),
    /// Could not even be spawned.
    NotSpawned(String),
}

/// Start one child with `attempt` and wait for it.
pub fn run_child(exe: &Path, args: &[String], attempt: Attempt, ready: &Path) -> Outcome {
    run_child_with(exe, args, attempt, ready, &[])
}

fn run_child_with(exe: &Path, args: &[String], attempt: Attempt, ready: &Path, extra_env: &[(&str, &str)]) -> Outcome {
    let _ = std::fs::remove_file(ready);
    let mut cmd = Command::new(exe);
    cmd.args(args).env(CHILD_ENV, "1").env("WGPU_BACKEND", attempt.backend).env(READY_ENV, ready);
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    if attempt.software {
        cmd.env(SOFTWARE_ENV, "1");
    } else {
        cmd.env_remove(SOFTWARE_ENV);
    }
    let status = match cmd.status() {
        Ok(s) => s,
        Err(e) => return Outcome::NotSpawned(e.to_string()),
    };
    let came_up = ready.exists();
    let _ = std::fs::remove_file(ready);
    if came_up || status.success() {
        Outcome::Ran(status.code().unwrap_or(if status.success() { 0 } else { 1 }))
    } else {
        Outcome::FailedToStart(status.to_string())
    }
}

/// The file that remembers the backend that worked.
pub fn memory_file(config_dir: Option<&Path>) -> Option<PathBuf> {
    config_dir.map(|d| d.join("graphics-backend.txt"))
}

/// Try each backend in turn. Returns the exit code to end with.
pub fn run(exe: &Path, args: &[String], config_dir: Option<&Path>) -> i32 {
    let memory = memory_file(config_dir);
    let remembered = memory.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).map(|s| s.trim().to_string());
    let ready = std::env::temp_dir().join(format!("deckrt-ready-{}", std::process::id()));
    let mut failures = vec![];
    for attempt in order(remembered.as_deref()) {
        log::info!("deckrt: starting with graphics backend {}", attempt.name());
        match run_child(exe, args, attempt, &ready) {
            Outcome::Ran(code) => {
                if let Some(p) = &memory {
                    let _ = std::fs::create_dir_all(p.parent().unwrap_or(Path::new(".")));
                    let _ = std::fs::write(p, attempt.name());
                }
                return code;
            }
            Outcome::FailedToStart(why) => {
                log::warn!("deckrt: graphics backend {} did not start ({why}); trying the next", attempt.name());
                failures.push(format!("{}: {why}", attempt.name()));
            }
            Outcome::NotSpawned(why) => {
                log::error!("deckrt: could not start itself again: {why}");
                failures.push(format!("{}: {why}", attempt.name()));
                break;
            }
        }
    }
    let text = format!(
        "deckrt could not open a window with any graphics backend.\n\nTried:\n  {}\n\nUpdate the graphics drivers (Intel and NVIDIA on this laptop), then start deckrt again.\nThe logs are in the logs folder next to this file: startup.log for the attempts, deckcraft.log for the last one.\n",
        failures.join("\n  ")
    );
    eprintln!("{text}");
    if let Some(dir) = config_dir {
        let p = dir.join("startup-failed.txt");
        let _ = std::fs::create_dir_all(dir);
        if std::fs::write(&p, &text).is_ok() {
            open_text_file(&p);
        }
    }
    2
}

/// Show a text file with the system's text editor, so the message is seen without a console.
fn open_text_file(p: &Path) {
    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("cmd").args(["/C", "start", "", &p.to_string_lossy()]).spawn();
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = p;
    }
}

/// In the child: the first frame is drawn, tell the supervisor.
pub fn mark_ready() {
    if let Some(p) = std::env::var_os(READY_ENV) {
        let _ = std::fs::write(p, b"ready");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supervision_is_off_for_children_overrides_and_help() {
        let on = |_: &str| None;
        let child = |k: &str| (k == CHILD_ENV).then(|| "1".to_string());
        let backend = |k: &str| (k == "WGPU_BACKEND").then(|| "gl".to_string());
        let forced = |k: &str| (k == "DECKRT_SUPERVISE").then(|| "1".to_string());
        assert!(!wanted(&[], child));
        assert!(!wanted(&[], backend));
        assert!(!wanted(&["--no-supervise".to_string()], forced));
        assert!(!wanted(&["--help".to_string()], forced));
        assert!(wanted(&["talk.pptx".to_string()], forced));
        assert_eq!(wanted(&[], on), cfg!(target_os = "windows"));
    }

    #[test]
    fn the_remembered_backend_goes_first() {
        let o = order(Some("gl"));
        assert_eq!(o[0].backend, "gl");
        assert_eq!(o.len(), 4);
        let o = order(Some("dx12 (software)"));
        assert!(o[0].software);
        let o = order(Some("nonsense"));
        assert_eq!(o[0], ATTEMPTS[0]);
    }

    /// The child is this test binary again, told to behave like the app (`DECKRT_FAKE_CHILD`).
    #[test]
    fn a_child_that_dies_before_its_first_frame_counts_as_failed_and_one_that_came_up_does_not() {
        if let Ok(mode) = std::env::var("DECKRT_FAKE_CHILD") {
            match mode.as_str() {
                "crash" => std::process::exit(3),
                "up-then-crash" => {
                    mark_ready();
                    std::process::exit(3);
                }
                _ => std::process::exit(0),
            }
        }
        let exe = std::env::current_exe().expect("exe");
        let dir = std::env::temp_dir().join(format!("deckrt-supervise-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let ready = dir.join("ready");
        let args = vec![
            "--exact".to_string(),
            "supervise::tests::a_child_that_dies_before_its_first_frame_counts_as_failed_and_one_that_came_up_does_not".to_string(),
        ];
        let run = |mode: &str, attempt: Attempt| run_child_with(&exe, &args, attempt, &ready, &[("DECKRT_FAKE_CHILD", mode)]);
        assert!(matches!(run("crash", ATTEMPTS[0]), Outcome::FailedToStart(_)));
        assert_eq!(run("up-then-crash", ATTEMPTS[1]), Outcome::Ran(3));
        assert_eq!(run("ok", ATTEMPTS[2]), Outcome::Ran(0));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
