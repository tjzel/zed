use std::collections::BTreeMap;
use std::ops::Range;

use collections::HashSet;
use editor::file_status_label_color;
use file_icons::FileIcons;
use git::repository::RepoPath;
use git::status::FileStatus;
use gpui::{
    Anchor, AnyElement, App, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable,
    MouseDownEvent, Point, ScrollStrategy, SharedString, Subscription, UniformListScrollHandle,
    Window, anchored, deferred, uniform_list,
};
use ui::{ContextMenu, DiffStat, ListItem, prelude::*};

use crate::git_status_icon;

pub struct DiffFileTree {
    entries: Vec<DiffTreeEntry>,
    rows: Vec<TreeRow>,
    visible_rows: Vec<usize>,
    collapsed_dirs: HashSet<SharedString>,
    active_path: Option<RepoPath>,
    focus_handle: FocusHandle,
    scroll_handle: UniformListScrollHandle,
    context_menu: Option<(Entity<ContextMenu>, Point<Pixels>, Subscription)>,
}

#[derive(Clone)]
pub struct DiffTreeEntry {
    pub repo_path: RepoPath,
    pub status: FileStatus,
    pub diff_stat: Option<git::status::DiffStat>,
}

pub enum DiffFileTreeEvent {
    OpenEntry {
        repo_path: RepoPath,
        target: OpenTarget,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum OpenTarget {
    /// Activated by clicking the entry; the consumer picks its own default.
    Default,
    /// The file itself, in a regular editor.
    File,
    /// A diff of just this file.
    SingleBuffer,
    /// The multibuffer containing every changed file, scrolled to this one.
    MultiBuffer,
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
            context_menu: None,
        }
    }

    pub fn set_entries(&mut self, mut entries: Vec<DiffTreeEntry>, cx: &mut Context<Self>) {
        entries.sort_by(|a, b| a.repo_path.cmp(&b.repo_path));
        if self.entries.len() == entries.len()
            && self
                .entries
                .iter()
                .zip(&entries)
                .all(|(old, new)| {
                    old.repo_path == new.repo_path
                        && old.status == new.status
                        && old.diff_stat.map(|stat| (stat.added, stat.deleted))
                            == new.diff_stat.map(|stat| (stat.added, stat.deleted))
                })
        {
            return;
        }
        self.entries = entries;
        self.rebuild_rows();
        cx.notify();
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Total added and deleted lines across every changed file.
    pub fn totals(&self) -> (u32, u32) {
        self.entries
            .iter()
            .filter_map(|entry| entry.diff_stat)
            .fold((0, 0), |(added, deleted), stat| {
                (added + stat.added, deleted + stat.deleted)
            })
    }

    pub fn set_active_path(&mut self, active_path: Option<RepoPath>, cx: &mut Context<Self>) {
        if self.active_path == active_path {
            return;
        }
        self.active_path = active_path;
        if let Some(active_path) = &self.active_path
            && let Some(visible_ix) = self.visible_rows.iter().position(|row_ix| {
                match &self.rows[*row_ix].kind {
                    RowKind::File { entry_ix } => self.entries[*entry_ix].repo_path == *active_path,
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
            if components.pop().is_none() {
                continue;
            }
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
            // Collapse chains of directories that hold nothing but one more
            // directory, so deep paths do not cost a row each.
            let mut name = name.clone();
            let mut child = child;
            while child.files.is_empty() && child.dirs.len() == 1 {
                let Some((child_name, grandchild)) = child.dirs.first_key_value() else {
                    break;
                };
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

    fn open_entry(&mut self, repo_path: RepoPath, target: OpenTarget, cx: &mut Context<Self>) {
        self.active_path = Some(repo_path.clone());
        cx.emit(DiffFileTreeEvent::OpenEntry { repo_path, target });
        cx.notify();
    }

    fn deploy_entry_context_menu(
        &mut self,
        repo_path: RepoPath,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let this = cx.entity().downgrade();
        let context_menu = ContextMenu::build(window, cx, move |context_menu, _, _| {
            let entry = |context_menu: ContextMenu, label: &'static str, target: OpenTarget| {
                let this = this.clone();
                let repo_path = repo_path.clone();
                context_menu.entry(label, None, move |_, cx| {
                    this.update(cx, |this, cx| {
                        this.open_entry(repo_path.clone(), target, cx);
                    })
                    .ok();
                })
            };
            let context_menu = entry(context_menu, "Open File", OpenTarget::File);
            let context_menu = entry(context_menu, "Open as Singlebuffer", OpenTarget::SingleBuffer);
            entry(context_menu, "Open as Multibuffer", OpenTarget::MultiBuffer)
        });

        let subscription = cx.subscribe_in(
            &context_menu,
            window,
            |this, _, _: &DismissEvent, window, cx| {
                if this.context_menu.as_ref().is_some_and(|context_menu| {
                    context_menu.0.focus_handle(cx).contains_focused(window, cx)
                }) {
                    cx.focus_self(window);
                }
                this.context_menu.take();
                cx.notify();
            },
        );
        self.context_menu = Some((context_menu, position, subscription));
        cx.notify();
    }

    fn render_items(&mut self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        range
            .filter_map(|ix| {
                let row = self.rows.get(*self.visible_rows.get(ix)?)?;
                Some(match &row.kind {
                    RowKind::Dir { path, name } => {
                        self.render_dir_row(ix, row.depth, path, name, cx)
                    }
                    RowKind::File { entry_ix } => {
                        self.render_file_row(ix, row.depth, self.entries.get(*entry_ix)?, cx)
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
                    .children(FileIcons::get_chevron_icon(expanded, cx).map(|icon| {
                        Icon::from_path(icon)
                            .size(IconSize::Small)
                            .color(Color::Muted)
                    }))
                    .children(
                        FileIcons::get_folder_icon(
                            expanded,
                            std::path::Path::new(name.as_ref()),
                            cx,
                        )
                        .map(|icon| {
                            Icon::from_path(icon)
                                .size(IconSize::Small)
                                .color(Color::Muted)
                        }),
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
        let label_color = file_status_label_color(Some(entry.status));
        let repo_path = entry.repo_path.clone();
        let secondary_repo_path = entry.repo_path.clone();
        ListItem::new(ix)
            .indent_level(depth + 1)
            .indent_step_size(px(12.))
            .toggle_state(is_active)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.open_entry(repo_path.clone(), OpenTarget::Default, cx);
            }))
            .on_secondary_mouse_down(cx.listener(
                move |this, event: &MouseDownEvent, window, cx| {
                    this.deploy_entry_context_menu(
                        secondary_repo_path.clone(),
                        event.position,
                        window,
                        cx,
                    );
                },
            ))
            .child(
                h_flex()
                    .gap_1()
                    .children(
                        FileIcons::get_icon(entry.repo_path.as_std_path(), cx).map(|icon| {
                            Icon::from_path(icon)
                                .size(IconSize::Small)
                                .color(Color::Muted)
                        }),
                    )
                    .child(git_status_icon(entry.status))
                    .child(
                        Label::new(file_name)
                            .color(label_color)
                            .when(entry.status.is_deleted(), Label::strikethrough)
                            .truncate(),
                    ),
            )
            .end_slot::<AnyElement>(entry.diff_stat.map(|stat| {
                DiffStat::new(("diff-stat", ix), stat.added as usize, stat.deleted as usize)
                    .into_any_element()
            }))
            .into_any_element()
    }
}

impl Focusable for DiffFileTree {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for DiffFileTree {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .key_context("DiffFileTree")
            .track_focus(&self.focus_handle)
            .size_full()
            .child(
                uniform_list(
                    "diff-file-tree",
                    self.visible_rows.len(),
                    cx.processor(|this, range: Range<usize>, _window, cx| {
                        this.render_items(range, cx)
                    }),
                )
                .flex_grow(1.)
                .track_scroll(&self.scroll_handle),
            )
            .children(self.context_menu.as_ref().map(|(menu, position, _)| {
                deferred(
                    anchored()
                        .position(*position)
                        .anchor(Anchor::TopLeft)
                        .child(menu.clone()),
                )
                .with_priority(1)
            }))
    }
}
