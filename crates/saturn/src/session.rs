//! The window as it was, per project, so Saturn opens where it left off: the repos it held, the
//! shape of the grid, and for each tab its folder, the name typed for it, and the agent's session
//! it was running (Claude Code's or Codex's).
//! Kept in `~/Library/Application Support/Saturn/sessions.json`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use saturn_core::agents::Agent;
use serde::{Deserialize, Serialize};

pub use saturn_core::workspaces::{SavedEditor, SavedFile};
pub type SavedGrid = saturn_core::workspaces::SavedGrid<SavedTab>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedTab {
    pub cwd: PathBuf,
    /// A name typed by hand for the tab.
    #[serde(default)]
    pub name: Option<String>,
    /// The agent's session that was running in it, offered for resuming.
    #[serde(default, alias = "claude_session")]
    pub session: Option<String>,
    /// Whose session it is; grids saved before Codex knew only Claude Code.
    #[serde(default)]
    pub agent: Option<Agent>,
}

fn file() -> Option<PathBuf> {
    crate::settings::data_dir().map(|dir| dir.join("sessions.json"))
}

fn read_all() -> HashMap<PathBuf, SavedGrid> {
    file()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// The grid saved for `project`, if any.
pub fn load(project: &Path) -> Option<SavedGrid> {
    read_all().remove(project).filter(|grid| grid.shape.tab_count() == grid.tabs.len())
}

/// Saves `project`'s grid, or forgets it when `grid` is `None` (the last tab closed).
pub fn save(project: &Path, grid: Option<SavedGrid>) {
    let Some(path) = file() else { return };
    if path.exists()
        && std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<HashMap<PathBuf, SavedGrid>>(&bytes).ok())
            .is_none()
    {
        eprintln!("saturn: refusing to overwrite unreadable session data");
        return;
    }
    let mut all = read_all();
    match grid {
        Some(grid) => all.insert(project.to_owned(), grid),
        None => all.remove(project),
    };
    if path.exists()
        && !path.with_extension("json.backup").exists()
        && std::fs::copy(&path, path.with_extension("json.backup")).is_err()
    {
        return;
    }
    if let Some(dir) = path.parent()
        && std::fs::create_dir_all(dir).is_ok()
    {
        let _ = write_json(&path, &all);
    }
}

/// The windows open when Saturn last quit, by the repo each was opened on, kept in `windows.json`
/// beside the grids so the next start opens them again, however Saturn is started (the Dock, an
/// update's relaunch). Automated runs leave the person's own list alone unless they have a data
/// folder of their own.
fn windows_file() -> Option<PathBuf> {
    let own = !crate::spike::is_automated() || std::env::var_os("SATURN_USER_DATA").is_some();
    own.then(crate::settings::data_dir).flatten().map(|dir| dir.join("windows.json"))
}

/// Adds a window's repo to those open at quit.
pub fn remember_open_window(project: &Path) {
    let Some(path) = windows_file() else { return };
    let mut open: Vec<PathBuf> =
        std::fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    if !open.iter().any(|known| known == project) {
        open.push(project.to_owned());
    }
    if let Ok(text) = serde_json::to_string_pretty(&open) {
        let _ = std::fs::write(path, text);
    }
}

/// The repos whose windows were open at the last quit, still there, and forgets them: this start
/// opens them, and the next quit writes down what's open then.
pub fn take_open_windows() -> Vec<PathBuf> {
    let Some(path) = windows_file() else { return Vec::new() };
    let open: Vec<PathBuf> =
        std::fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    let _ = std::fs::remove_file(path);
    open.into_iter().filter(|project| project.is_dir()).collect()
}

/// What to type into a restored tab to resume its agent's session: typed, not run, so resuming is
/// one Enter and never a surprise.
pub fn resume_command(agent: Agent, session: &str) -> Option<String> {
    // Session ids are UUIDs; anything else is not typed into a shell.
    session.chars().all(|c| c.is_ascii_hexdigit() || c == '-').then(|| agent.resume(session))
}

pub fn write_json(path: &Path, value: &impl Serialize) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    std::fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_uuid_like_sessions_are_offered() {
        assert_eq!(
            resume_command(Agent::Claude, "0b6f7c2e-1a2b-4c3d-9e8f-001122334455").as_deref(),
            Some("claude --resume 0b6f7c2e-1a2b-4c3d-9e8f-001122334455")
        );
        assert_eq!(
            resume_command(Agent::Codex, "019d7c54-01b0-7992-acd4-66566288cc3e").as_deref(),
            Some("codex resume 019d7c54-01b0-7992-acd4-66566288cc3e")
        );
        assert_eq!(resume_command(Agent::Claude, "x; rm -rf ~"), None);
    }

    #[test]
    fn a_tab_saved_before_codex_resumes_its_claude_session() {
        let tab: SavedTab =
            serde_json::from_str(r#"{"cwd":"/code/acme-web","claude_session":"0b6f7c2e"}"#).expect("reads");
        assert_eq!((tab.session.as_deref(), tab.agent), (Some("0b6f7c2e"), None));
    }
}
