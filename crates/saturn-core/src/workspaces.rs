//! Named workspace metadata, independent of windows and running terminals.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceEntry {
    pub root: PathBuf,
    pub name: String,
    #[serde(default)]
    pub color: usize,
    #[serde(default)]
    pub active: bool,
}

pub fn load(path: &Path) -> Vec<WorkspaceEntry> {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn save(path: &Path, entries: &[WorkspaceEntry]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if path.exists() {
        let _: Vec<WorkspaceEntry> = serde_json::from_slice(&std::fs::read(path)?)?;
    }
    if path.exists() && !path.with_extension("json.backup").exists() {
        std::fs::copy(path, path.with_extension("json.backup"))?;
    }
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, serde_json::to_vec_pretty(entries)?)?;
    std::fs::rename(temporary, path)
}

/// Moves a project without changing the workspace's persistence anchor.
pub fn reorder(projects: &mut Vec<PathBuf>, source: &Path, target: &Path) -> bool {
    let (Some(from), Some(to)) = (projects.iter().position(|p| p == source), projects.iter().position(|p| p == target))
    else {
        return false;
    };
    if from == to {
        return false;
    }
    let project = projects.remove(from);
    projects.insert(to, project);
    true
}

use crate::layout::Shape;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[expect(clippy::struct_excessive_bools, reason = "independent persisted panel and navigation switches")]
pub struct SavedGrid<T> {
    /// The other repos the window held, after the one it's saved under.
    #[serde(default)]
    pub projects: Vec<PathBuf>,
    pub shape: Shape,
    /// One per tab, in the order the shape lists them.
    pub tabs: Vec<T>,
    /// Display order, including the workspace anchor; absent in older sessions.
    #[serde(default)]
    pub project_order: Vec<PathBuf>,
    #[serde(default)]
    pub selected_project: Option<PathBuf>,
    #[serde(default = "enabled")]
    pub sidebar_open: bool,
    #[serde(default = "enabled")]
    pub repo_tabs_open: bool,
    #[serde(default = "enabled")]
    pub editor_open: bool,
    #[serde(default)]
    pub tickets_open: bool,
    #[serde(default = "enabled")]
    pub follow_pane: bool,
    #[serde(default)]
    pub changes_open: bool,
    #[serde(default)]
    pub maximized_terminal: Option<usize>,
    #[serde(default = "sidebar_width")]
    pub sidebar_width: f32,
    #[serde(default = "editor_height")]
    pub editor_height: f32,
    #[serde(default)]
    pub editor: SavedEditor,
}

fn enabled() -> bool {
    true
}
fn sidebar_width() -> f32 {
    270.0
}
fn editor_height() -> f32 {
    260.0
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedEditor {
    pub files: Vec<SavedFile>,
    pub active: Option<PathBuf>,
    pub maximized: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedFile {
    pub path: PathBuf,
    /// Unsaved buffers live only in Saturn's session file, never written over source files.
    pub draft: Option<String>,
    pub base: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_round_trip_and_original_backup() {
        let dir = tempfile::tempdir().expect("data directory");
        let file = dir.path().join("workspaces.json");
        let legacy = r#"[{"root":"/project","name":"Project"}]"#;
        std::fs::write(&file, legacy).expect("legacy data");
        let mut entries = load(&file);
        assert_eq!(entries[0].color, 0);
        entries.push(WorkspaceEntry { root: PathBuf::new(), name: "Empty".into(), color: 3, active: true });
        save(&file, &entries).expect("save");
        assert_eq!(load(&file).len(), 2);
        assert!(load(&file)[1].root.as_os_str().is_empty());
        assert_eq!(std::fs::read_to_string(file.with_extension("json.backup")).expect("backup"), legacy);
    }

    #[test]
    fn invalid_metadata_is_never_overwritten() {
        let dir = tempfile::tempdir().expect("directory");
        let file = dir.path().join("workspaces.json");
        std::fs::write(&file, "broken").expect("fixture");
        assert!(save(&file, &[]).is_err());
        assert_eq!(std::fs::read_to_string(file).expect("original"), "broken");
    }

    #[test]
    fn reorders_projects_both_ways_and_rejects_missing_targets() {
        let mut projects: Vec<_> = ["a", "b", "c"].map(PathBuf::from).into();
        assert!(reorder(&mut projects, Path::new("a"), Path::new("c")));
        assert_eq!(projects, ["b", "c", "a"].map(PathBuf::from));
        assert!(reorder(&mut projects, Path::new("a"), Path::new("b")));
        assert!(!reorder(&mut projects, Path::new("missing"), Path::new("a")));
        assert_eq!(projects, ["a", "b", "c"].map(PathBuf::from));
    }

    #[test]
    fn grid_defaults_and_drafts_survive_serialization() {
        let layout = crate::layout::Layout::default();
        let legacy = serde_json::json!({"projects": [], "shape": layout.shape(), "tabs": []});
        let mut grid: SavedGrid<serde_json::Value> = serde_json::from_value(legacy).expect("legacy grid");
        assert!(grid.sidebar_open);
        assert_eq!(grid.sidebar_width, 270.);
        grid.editor.files.push(SavedFile {
            path: "/project/missing.rs".into(),
            draft: Some("unsaved".into()),
            base: Some("original".into()),
        });
        let restored: SavedGrid<serde_json::Value> =
            serde_json::from_slice(&serde_json::to_vec(&grid).expect("serialize")).expect("restore");
        assert_eq!(restored, grid);
    }
}
