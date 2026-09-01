use std::collections::BTreeMap;
use std::ops::Range;

use collections::HashSet;
use file_icons::FileIcons;
use git::repository::RepoPath;
use git::status::FileStatus;
use gpui::{
    AnyElement, App, Context, EventEmitter, FocusHandle, Focusable, ScrollStrategy, SharedString,
    UniformListScrollHandle, Window, uniform_list,
};
use multi_buffer::PathKey;
use ui::{ListItem, prelude::*};

use crate::git_status_icon;

pub struct DiffFileTree {
    entries: Vec<DiffTreeEntry>,
    rows: Vec<TreeRow>,
    visible_rows: Vec<usize>,
    collapsed_dirs: HashSet<SharedString>,
    active_path: Option<RepoPath>,
    focus_handle: FocusHandle,
    scroll_handle: UniformListScrollHandle,
}

#[derive(Clone)]
pub struct DiffTreeEntry {
    pub path_key: PathKey,
    pub repo_path: RepoPath,
    pub status: FileStatus,
}

pub enum DiffFileTreeEvent {
    OpenEntry { path_key: PathKey },
}

impl EventEmitter<DiffFileTreeEvent> for DiffFileTree {}

struct TreeRow {
    depth: usize,
    kind: RowKind,
}

enum RowKind {
    Dir {
        path: SharedString,
        name: SharedString,
    },
    File {
        entry_ix: usize,
    },
}

#[derive(Default)]
struct DirNode {
    dirs: BTreeMap<String, DirNode>,
    files: Vec<usize>,
}

impl DiffFileTree {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            entries: Vec::new(),
            rows: Vec::new(),
            visible_rows: Vec::new(),
            collapsed_dirs: HashSet::default(),
            active_path: None,
            focus_handle: cx.focus_handle(),
            scroll_handle: UniformListScrollHandle::new(),
        }
    }

    pub fn set_entries(&mut self, mut entries: Vec<DiffTreeEntry>, cx: &mut Context<Self>) {
        entries.sort_by(|a, b| a.repo_path.cmp(&b.repo_path));
        self.entries = entries;
        self.rebuild_rows();
        cx.notify();
    }

    pub fn set_active_path(&mut self, active_path: Option<RepoPath>, cx: &mut Context<Self>) {
        if self.active_path == active_path {
            return;
        }
        self.active_path = active_path;
        if let Some(active_path) = &self.active_path
            && let Some(visible_ix) = self.visible_rows.iter().position(|row_ix| {
                match &self.rows[*row_ix].kind {
                    RowKind::File { entry_ix } => {
                        self.entries[*entry_ix].repo_path == *active_path
                    }
                    RowKind::Dir { .. } => false,
                }
            })
        {
            self.scroll_handle
                .scroll_to_item(visible_ix, ScrollStrategy::Center);
        }
        cx.notify();
    }

    fn toggle_dir(&mut self, path: &SharedString, cx: &mut Context<Self>) {
        if !self.collapsed_dirs.remove(path) {
            self.collapsed_dirs.insert(path.clone());
        }
        self.rebuild_visible_rows();
        cx.notify();
    }

    fn rebuild_rows(&mut self) {
        let mut root = DirNode::default();
        for (entry_ix, entry) in self.entries.iter().enumerate() {
            let mut components: Vec<&str> = entry.repo_path.components().collect();
            let Some(_file_name) = components.pop() else {
                continue;
            };
            let mut node = &mut root;
            for component in components {
                node = node.dirs.entry(component.to_string()).or_default();
            }
            node.files.push(entry_ix);
        }

        self.rows.clear();
        Self::push_dir_contents(&root, "", 0, &mut self.rows);
        self.rebuild_visible_rows();
    }

    fn push_dir_contents(node: &DirNode, prefix: &str, depth: usize, rows: &mut Vec<TreeRow>) {
        for (name, child) in &node.dirs {
            let mut name = name.clone();
            let mut child = child;
            while child.files.is_empty() && child.dirs.len() == 1 {
                let (child_name, grandchild) = child.dirs.first_key_value().unwrap();
                name = format!("{name}/{child_name}");
                child = grandchild;
            }
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            rows.push(TreeRow {
                depth,
                kind: RowKind::Dir {
                    path: path.clone().into(),
                    name: name.into(),
                },
            });
            Self::push_dir_contents(child, &path, depth + 1, rows);
        }
        for entry_ix in &node.files {
            rows.push(TreeRow {
                depth,
                kind: RowKind::File {
                    entry_ix: *entry_ix,
                },
            });
        }
    }

    fn rebuild_visible_rows(&mut self) {
        self.visible_rows.clear();
        let mut skip_below_depth: Option<usize> = None;
        for (row_ix, row) in self.rows.iter().enumerate() {
            if let Some(depth) = skip_below_depth {
                if row.depth > depth {
                    continue;
                }
                skip_below_depth = None;
            }
            self.visible_rows.push(row_ix);
            if let RowKind::Dir { path, .. } = &row.kind
                && self.collapsed_dirs.contains(path)
            {
                skip_below_depth = Some(row.depth);
            }
        }
    }

    fn render_items(&mut self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        range
            .filter_map(|ix| {
                let row = self.rows.get(*self.visible_rows.get(ix)?)?;
                Some(match &row.kind {
                    RowKind::Dir { path, name } => self.render_dir_row(ix, row.depth, path, name, cx),
                    RowKind::File { entry_ix } => {
                        let entry = self.entries.get(*entry_ix)?;
                        self.render_file_row(ix, row.depth, entry, cx)
                    }
                })
            })
            .collect()
    }

    fn render_dir_row(
        &self,
        ix: usize,
        depth: usize,
        path: &SharedString,
        name: &SharedString,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let expanded = !self.collapsed_dirs.contains(path);
        let path = path.clone();
        ListItem::new(ix)
            .indent_level(depth)
            .indent_step_size(px(12.))
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_dir(&path, cx)))
            .child(
                h_flex()
                    .gap_1()
                    .children(
                        FileIcons::get_chevron_icon(expanded, cx)
                            .map(|icon| Icon::from_path(icon).size(IconSize::Small).color(Color::Muted)),
                    )
                    .children(
                        FileIcons::get_folder_icon(expanded, std::path::Path::new(name.as_ref()), cx)
                            .map(|icon| Icon::from_path(icon).size(IconSize::Small).color(Color::Muted)),
                    )
                    .child(Label::new(name.clone()).color(Color::Muted).truncate()),
            )
            .into_any_element()
    }

    fn render_file_row(
        &self,
        ix: usize,
        depth: usize,
        entry: &DiffTreeEntry,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let file_name: SharedString = entry
            .repo_path
            .file_name()
            .unwrap_or_default()
            .to_string()
            .into();
        let is_active = self.active_path.as_ref() == Some(&entry.repo_path);
        let label_color = Self::status_label_color(entry.status);
        let path_key = entry.path_key.clone();
        let repo_path = entry.repo_path.clone();
        ListItem::new(ix)
            .indent_level(depth + 1)
            .indent_step_size(px(12.))
            .toggle_state(is_active)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.active_path = Some(repo_path.clone());
                cx.emit(DiffFileTreeEvent::OpenEntry {
                    path_key: path_key.clone(),
                });
                cx.notify();
            }))
            .child(
                h_flex()
                    .gap_1()
                    .children(
                        FileIcons::get_icon(entry.repo_path.as_std_path(), cx)
                            .map(|icon| Icon::from_path(icon).size(IconSize::Small).color(Color::Muted)),
                    )
                    .child(git_status_icon(entry.status))
                    .child(
                        Label::new(file_name)
                            .color(label_color)
                            .when(entry.status.is_deleted(), Label::strikethrough)
                            .truncate(),
                    ),
            )
            .into_any_element()
    }

    fn status_label_color(status: FileStatus) -> Color {
        if status.is_conflicted() {
            Color::VersionControlConflict
        } else if status.is_created() {
            Color::VersionControlAdded
        } else if status.is_modified() {
            Color::VersionControlModified
        } else if status.is_deleted() {
            Color::Disabled
        } else {
            Color::Default
        }
    }
}

impl Focusable for DiffFileTree {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for DiffFileTree {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let file_count = self.entries.len();
        let header: SharedString = if file_count == 1 {
            "1 changed file".into()
        } else {
            format!("{file_count} changed files").into()
        };

        v_flex()
            .key_context("DiffFileTree")
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(cx.theme().colors().panel_background)
            .child(
                h_flex()
                    .px_2()
                    .py_1()
                    .border_b_1()
                    .border_color(cx.theme().colors().border)
                    .child(Label::new(header).size(LabelSize::Small).color(Color::Muted)),
            )
            .child(
                uniform_list(
                    "diff-file-tree",
                    self.visible_rows.len(),
                    cx.processor(|this, range: Range<usize>, _window, cx| {
                        this.render_items(range, cx)
                    }),
                )
                .flex_grow()
                .track_scroll(&self.scroll_handle),
            )
    }
}
