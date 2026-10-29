//! The window's root: the welcome screen until a project is picked, then the workspace for it.
//! Started from a terminal inside a folder, Saturn skips the welcome and opens that folder.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::{
    AppContext as _, ClickEvent, Context, Entity, FocusHandle, FontWeight, InteractiveElement as _, IntoElement,
    MouseButton, ParentElement as _, PathPromptOptions, PromptLevel, Render, StatefulInteractiveElement as _,
    Styled as _, Window, div, prelude::FluentBuilder as _, px,
};
use saturn_design::{color, radius, space};

use crate::chrome;
use crate::settings;
use crate::spike;
use crate::theme::{self, len};
use crate::workspace::{OpenProject, Placement, Workspace};

const WORKSPACE_COLORS: [saturn_design::Token; 8] = [
    color::INK_MUTED,
    color::RED,
    color::ORANGE,
    color::TERM_YELLOW,
    color::TERM_GREEN,
    color::BLUE,
    color::TERM_MAGENTA,
    color::TERM_CYAN,
];

use saturn_core::workspaces::WorkspaceEntry;

fn workspace_file() -> Option<PathBuf> {
    if spike::is_automated() && std::env::var_os("SATURN_USER_DATA").is_none() {
        return None;
    }
    settings::data_dir().map(|p| p.join("workspaces.json"))
}
fn load_workspaces() -> Vec<WorkspaceEntry> {
    workspace_file().map_or_else(Vec::new, |p| saturn_core::workspaces::load(&p))
}
fn save_workspaces(entries: &[WorkspaceEntry]) {
    if let Some(p) = workspace_file() {
        let _ = saturn_core::workspaces::save(&p, entries);
    }
}

gpui_kit::actions!(saturn, [CreateWorkspace, NextWorkspace, PreviousWorkspace, EditWorkspace, RemoveWorkspace]);

pub struct AppView {
    workspace: Option<Entity<Workspace>>,
    entries: Vec<WorkspaceEntry>,
    open_workspaces: HashMap<PathBuf, Entity<Workspace>>,
    focus: FocusHandle,
    editing: Option<usize>,
    active: Option<usize>,
    creating: Option<Entity<InputState>>,
    new_color: usize,
}

impl AppView {
    pub fn new(project: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut view = Self {
            workspace: None,
            entries: load_workspaces(),
            open_workspaces: HashMap::new(),
            focus: cx.focus_handle(),
            editing: None,
            active: None,
            creating: None,
            new_color: 0,
        };
        spike::start_window(window, cx);
        let weak = cx.weak_entity();
        window.on_window_should_close(cx, move |_, cx| {
            let _ = weak.update(cx, |this, cx| {
                for workspace in this.open_workspaces.values() {
                    workspace.read(cx).save_session(cx);
                    workspace.update(cx, Workspace::stop_terminals);
                }
            });
            true
        });
        cx.on_app_quit(|this, cx| {
            for workspace in this.open_workspaces.values() {
                workspace.read(cx).save_session(cx);
            }
            if let Some(workspace) = &this.workspace {
                crate::session::remember_open_window(workspace.read(cx).session_anchor());
            }
            async {}
        })
        .detach();

        if let Some(project) = project {
            view.open(&project, window, cx);
        } else if let Some(index) = view.entries.iter().position(|entry| entry.active) {
            view.switch_workspace(index, window, cx);
        } else {
            window.focus(&view.focus, cx);
        }
        view
    }

    /// Open or return to a workspace without stopping its terminals.
    fn open(&mut self, project: &Path, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(current) = &self.workspace {
            current.read(cx).save_session(cx);
            let anchor = current.read(cx).session_anchor().to_owned();
            if let Some((old, _)) = self
                .open_workspaces
                .iter()
                .find(|(_, value)| *value == current)
                .map(|(key, value)| (key.clone(), value.clone()))
                && old != anchor
            {
                if let Some(entry) = self.entries.iter_mut().find(|entry| entry.root == old) {
                    entry.root.clone_from(&anchor);
                }
                if let Some(entity) = self.open_workspaces.remove(&old) {
                    self.open_workspaces.insert(anchor, entity);
                }
            }
        }
        let project = project.to_owned();
        if !self.entries.iter().any(|entry| entry.root == project) {
            let name = project
                .file_name()
                .map_or_else(|| project.display().to_string(), |name| name.to_string_lossy().into_owned());
            self.entries.push(WorkspaceEntry { root: project.clone(), name, color: 0, active: false });
        }
        self.active = self.entries.iter().position(|entry| entry.root == project);
        for (index, entry) in self.entries.iter_mut().enumerate() {
            entry.active = Some(index) == self.active;
        }
        save_workspaces(&self.entries);
        settings::remember_project(&project);
        let workspace = if let Some(workspace) = self.open_workspaces.get(&project) {
            workspace.clone()
        } else {
            let root = project.clone();
            let workspace = cx.new(|cx| {
                let mut workspace = Workspace::new(root, window, cx);
                match spike::panes() {
                    Some(panes) => {
                        for index in 0..panes {
                            workspace.open_terminal(spike::placement(index, panes), window, cx);
                        }
                    }
                    None => {
                        if !workspace.restore(window, cx) {
                            workspace.open_terminal(Placement::Tab, window, cx);
                        }
                    }
                }
                spike::start(&workspace, window, cx);
                workspace
            });
            cx.observe(&workspace, |this, workspace, cx| {
                let anchor = workspace.read(cx).session_anchor().to_owned();
                let old = this.open_workspaces.iter().find(|(_, item)| **item == workspace).map(|(key, _)| key.clone());
                if let Some(old) = old
                    && old != anchor
                {
                    if let Some(entry) = this.entries.iter_mut().find(|entry| entry.root == old) {
                        entry.root.clone_from(&anchor);
                    }
                    if let Some(entity) = this.open_workspaces.remove(&old) {
                        this.open_workspaces.insert(anchor, entity);
                    }
                    save_workspaces(&this.entries);
                }
                cx.notify();
            })
            .detach();
            self.open_workspaces.insert(project, workspace.clone());
            workspace
        };
        workspace.update(cx, |workspace, cx| workspace.activate_workspace(window, cx));
        self.workspace = Some(workspace);
        cx.notify();
    }

    fn edit_workspace(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.entries.get(index) else { return };
        let name = entry.name.clone();
        self.new_color = entry.color;
        self.editing = Some(index);
        let input = cx.new(|cx| InputState::new(window, cx).default_value(name));
        self.creating = Some(input.clone());
        input.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    fn confirm_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(index), Some(input)) = (self.editing, &self.creating) else { return };
        let name = input.read(cx).value().trim().to_owned();
        if name.is_empty() {
            return;
        }
        let color = self.new_color;
        let Some(original) = self.entries.get(index).map(|entry| entry.name.clone()) else { return };
        let message = format!("Save changes to {original}?");
        let answer = window.prompt(
            PromptLevel::Info,
            &message,
            Some("The workspace name and color will change."),
            &["Save", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await == Ok(0) {
                let _ = this.update(cx, |this, cx| {
                    if let Some(entry) = this.entries.get_mut(index) {
                        entry.name = name;
                        entry.color = color;
                    }
                    save_workspaces(&this.entries);
                    this.creating = None;
                    this.editing = None;
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn confirm_remove(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.entries.get(index) else { return };
        let name = entry.name.clone();
        let message = format!("Remove workspace {name}?");
        let answer = window.prompt(
            PromptLevel::Warning,
            &message,
            Some("Its running terminals will stop. Project files will not be deleted."),
            &["Remove", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await == Ok(0) {
                let _ = this.update_in(cx, |this, window, cx| this.remove_workspace(index, window, cx));
            }
        })
        .detach();
    }

    fn remove_workspace(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.entries.len() {
            return;
        }
        let entry = self.entries.remove(index);
        if let Some(workspace) = self.open_workspaces.remove(&entry.root) {
            workspace.read(cx).save_session(cx);
            workspace.update(cx, Workspace::stop_terminals);
        }
        let was_active = self.active == Some(index);
        if was_active {
            self.workspace = None;
            crate::workspace::hide_workspace(window);
            self.active = None;
            if self.entries.is_empty() {
                window.focus(&self.focus, cx);
            } else {
                self.switch_workspace(index.min(self.entries.len() - 1), window, cx);
            }
        } else if let Some(active) = self.active
            && active > index
        {
            self.active = Some(active - 1);
        }
        self.creating = None;
        self.editing = None;
        save_workspaces(&self.entries);
        cx.notify();
    }

    fn start_create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Workspace name"));
        cx.subscribe_in(&input, window, |this, _, event, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.create_workspace(window, cx);
            }
        })
        .detach();
        self.creating = Some(input.clone());
        self.editing = None;
        self.new_color = 0;
        input.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    fn create_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(input) = &self.creating else { return };
        if self.editing.is_some() {
            self.confirm_edit(window, cx);
            return;
        }
        let name = input.read(cx).value().trim().to_owned();
        if name.is_empty() {
            return;
        }
        self.entries.push(WorkspaceEntry { root: PathBuf::new(), name, color: self.new_color, active: false });
        self.creating = None;
        self.switch_workspace(self.entries.len() - 1, window, cx);
    }

    fn switch_workspace(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.entries.get(index).map(|entry| entry.root.clone()) else { return };
        if root.as_os_str().is_empty() || !root.is_dir() {
            if let Some(current) = &self.workspace {
                current.read(cx).save_session(cx);
            }
            self.workspace = None;
            crate::workspace::hide_workspace(window);
            self.active = Some(index);
            for (i, entry) in self.entries.iter_mut().enumerate() {
                entry.active = i == index;
            }
            save_workspaces(&self.entries);
            window.focus(&self.focus, cx);
            cx.notify();
        } else {
            self.open(&root, window, cx);
        }
    }

    fn open_project(&mut self, project: &Path, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.active
            && (self.entries[index].root.as_os_str().is_empty() || !self.entries[index].root.is_dir())
        {
            if self.entries.iter().any(|entry| entry.root == project) {
                self.switch_workspace(
                    self.entries.iter().position(|entry| entry.root == project).expect("existing workspace"),
                    window,
                    cx,
                );
                return;
            }
            project.clone_into(&mut self.entries[index].root);
        }
        self.open(project, window, cx);
    }

    fn pick_folder(window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open Project".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else { return };
            let Some(project) = paths.into_iter().next() else { return };
            let _ = this.update_in(cx, |this, window, cx| this.open_project(&project, window, cx));
        })
        .detach();
    }

    fn render_recent(recent: &[PathBuf], cx: &mut Context<Self>) -> impl IntoElement {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        div().flex().flex_col().p(len(space::SPACE_1)).children(recent.iter().enumerate().map(|(index, project)| {
            let name =
                project.file_name().map_or_else(|| project.display().to_string(), |n| n.to_string_lossy().into_owned());
            let folder = match home.as_deref().and_then(|home| project.parent()?.strip_prefix(home).ok()) {
                Some(rest) => format!("~/{}", rest.display()),
                None => project.parent().map_or_else(String::new, |parent| parent.display().to_string()),
            };
            let project = project.clone();
            div()
                .id(("recent", index))
                .flex()
                .items_center()
                .gap(len(space::SPACE_3))
                .px(len(space::SPACE_4))
                .py(len(space::SPACE_3))
                .rounded(len(radius::RADIUS_SM))
                .cursor_pointer()
                .hover(|s| s.bg(theme::fill_hover()))
                .child(chrome::icon("folder", 16., theme::color(color::INK_MUTED)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .text_size(px(15.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme::color(color::INK))
                                .truncate()
                                .child(name),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(theme::color(color::INK_FAINT))
                                .truncate()
                                .child(folder),
                        ),
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.open_project(&project, window, cx)))
        }))
    }

    fn render_welcome(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let recent = settings::load().recent();
        let open = chrome::button("open-folder", "Open folder…", Some("⌘O"), true)
            .on_click(cx.listener(|_, _, window, cx| Self::pick_folder(window, cx)));
        div()
            .id("welcome")
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .key_context("Welcome")
            .on_action(cx.listener(|_, _: &OpenProject, window, cx| Self::pick_folder(window, cx)))
            .child(
                div().flex_1().flex().items_center().justify_center().pb(px(48.)).child(
                    div()
                        .w(px(440.))
                        .flex()
                        .flex_col()
                        .gap(len(space::SPACE_6))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(len(space::SPACE_2))
                                .child(chrome::headline("Open a", "project", 28.))
                                .child(div().text_size(px(14.)).text_color(theme::color(color::INK_MUTED)).child(
                                    if recent.is_empty() {
                                        "Pick the folder you want your agents to work in."
                                    } else {
                                        "Pick up where you left off, or open another folder."
                                    },
                                )),
                        )
                        .children((!recent.is_empty()).then(|| chrome::card().child(Self::render_recent(&recent, cx))))
                        .child(div().flex().child(open)),
                ),
            )
    }
}

impl AppView {
    fn render_creation(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        self.creating.as_ref().map(|input| {
            let mut colors = div().flex().items_center().gap(px(8.));
            for (index, color) in WORKSPACE_COLORS.into_iter().enumerate() {
                colors = colors.child(
                    div()
                        .id(("new-workspace-color", index))
                        .size(px(22.))
                        .rounded_full()
                        .bg(theme::color(color))
                        .when(index == self.new_color, |dot| dot.border_2().border_color(theme::color(color::INK)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.new_color = index;
                            cx.notify();
                        })),
                );
            }
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(12.))
                .px(px(16.))
                .py(px(10.))
                .bg(theme::color(color::GROUND))
                .child(div().w(px(200.)).child(Input::new(input).appearance(false)))
                .child(colors)
                .child(
                    chrome::button(
                        "create-workspace",
                        if self.editing.is_some() { "Save changes" } else { "Create workspace" },
                        None,
                        true,
                    )
                    .on_click(cx.listener(|this, _, window, cx| this.create_workspace(window, cx))),
                )
                .child(chrome::button("cancel-workspace", "Cancel", None, false).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.creating = None;
                        this.editing = None;
                        cx.notify();
                    },
                )))
        })
    }

    fn render_workspace_tiles(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut tiles =
            div().id("workspace-tiles").flex().items_center().gap(px(5.)).flex_1().min_w_0().overflow_x_scroll();
        for (index, entry) in self.entries.iter().enumerate() {
            let name = entry.name.clone();
            let selected = self.active == Some(index);
            let color = WORKSPACE_COLORS[entry.color.min(WORKSPACE_COLORS.len() - 1)];
            tiles = tiles.child(
                chrome::track_item(("workspace", index), selected)
                    .flex_none()
                    .px(px(12.))
                    .gap(px(7.))
                    .bg(theme::color(color).opacity(if selected { 0.35 } else { 0.16 }))
                    .child(div().size(px(10.)).rounded_full().bg(theme::color(color)))
                    .child(name)
                    .child(
                        div()
                            .id(("workspace-edit", index))
                            .ml(px(4.))
                            .rounded(px(4.))
                            .hover(|style| style.bg(theme::fill_control()))
                            .child(chrome::icon("pencil", 12., theme::color(color::INK_MUTED)))
                            .tooltip(chrome::tip("Edit workspace"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.edit_workspace(index, window, cx);
                            })),
                    )
                    .child(
                        div()
                            .id(("workspace-remove", index))
                            .rounded(px(4.))
                            .hover(|style| style.bg(theme::fill_control()))
                            .child(chrome::icon("x", 12., theme::color(color::INK_MUTED)))
                            .tooltip(chrome::tip("Remove workspace"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.confirm_remove(index, window, cx);
                            })),
                    )
                    .tooltip(chrome::tip("Switch workspace; right-click to edit"))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.edit_workspace(index, window, cx);
                        }),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| this.switch_workspace(index, window, cx))),
            );
        }
        tiles
    }
}

impl Render for AppView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match &self.workspace {
            Some(workspace) => workspace.clone().into_any_element(),
            None => self.render_welcome(cx).into_any_element(),
        };

        let tiles = self.render_workspace_tiles(cx);
        let bar = div()
            .flex_none()
            .h(px(48.))
            .flex()
            .items_center()
            // Native traffic lights extend past 76 pt; reserve their full width plus breathing room.
            .pl(px(96.))
            .pr(px(8.))
            .gap(px(6.))
            .bg(theme::color(color::GROUND))
            .text_color(theme::color(color::INK))
            .child(tiles)
            .child(
                chrome::icon_button("add-workspace", "plus", true)
                    .tooltip(chrome::tip("Add workspace"))
                    .on_click(cx.listener(|this, _, window, cx| this.start_create(window, cx))),
            );
        let creation = self.render_creation(cx);
        let backdrop = gpui_kit::linear_gradient(
            155.,
            gpui_kit::linear_color_stop(theme::color(color::BACKDROP), 0.),
            gpui_kit::linear_color_stop(theme::color(color::BACKDROP_DEEP), 1.),
        )
        .color_space(gpui_kit::ColorSpace::Oklab);
        let root = div()
            .bg(backdrop)
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .child(bar)
            .children(creation)
            .child(div().flex_1().min_h_0().child(content));
        root.on_action(cx.listener(|this, _: &CreateWorkspace, window, cx| this.start_create(window, cx)))
            .on_action(cx.listener(|this, _: &NextWorkspace, window, cx| {
                if !this.entries.is_empty() {
                    this.switch_workspace((this.active.unwrap_or(0) + 1) % this.entries.len(), window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &PreviousWorkspace, window, cx| {
                if !this.entries.is_empty() {
                    this.switch_workspace(
                        (this.active.unwrap_or(0) + this.entries.len() - 1) % this.entries.len(),
                        window,
                        cx,
                    );
                }
            }))
            .on_action(cx.listener(|this, _: &EditWorkspace, window, cx| {
                if let Some(i) = this.active {
                    this.edit_workspace(i, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &RemoveWorkspace, window, cx| {
                if let Some(i) = this.active {
                    this.confirm_remove(i, window, cx);
                }
            }))
    }
}
