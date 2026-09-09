use std::sync::Arc;

use anyhow::Result;
use git::repository::RepoPath;
use gpui::{
    App, AsyncWindowContext, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement,
    Render, SharedString, Subscription, WeakEntity, Window, actions,
};
use project::{
    Fs, Project, ProjectPath,
    git_store::{
        Repository,
        diff_buffer_list::{BranchDiffEvent, DiffBase, DiffBufferList},
    },
};
use settings::Settings as _;
use ui::{Button, ButtonCommon as _, Clickable as _, Tooltip, prelude::*};
use workspace::Workspace;
use workspace::dock::{DockPosition, Panel, PanelEvent};

use crate::branch_diff::BranchDiff;
use crate::diff_file_tree::{DiffFileTree, DiffFileTreeEvent, DiffTreeEntry, OpenTarget};

actions!(
    compare_panel,
    [
        /// Toggles focus on the compare panel.
        ToggleFocus
    ]
);

pub fn register(workspace: &mut Workspace) {
    workspace.register_action(|workspace, _: &ToggleFocus, window, cx| {
        workspace.toggle_panel_focus::<ComparePanel>(window, cx);
    });
}

#[derive(Debug, Clone, PartialEq, settings::RegisterSetting)]
pub struct ComparePanelSettings {
    pub dock: DockPosition,
    pub default_width: Pixels,
}

impl settings::Settings for ComparePanelSettings {
    fn from_settings(content: &settings::SettingsContent) -> Self {
        let compare_panel = content.compare_panel.clone().unwrap();
        Self {
            dock: compare_panel.dock.unwrap().into(),
            default_width: px(compare_panel.default_width.unwrap()),
        }
    }
}

pub struct ComparePanel {
    project: Entity<Project>,
    workspace: WeakEntity<Workspace>,
    diff_buffer_list: Option<Entity<DiffBufferList>>,
    base_ref: Option<SharedString>,
    file_tree: Entity<DiffFileTree>,
    fs: Arc<dyn Fs>,
    focus_handle: FocusHandle,
    _tree_subscription: Subscription,
    _diff_subscription: Option<Subscription>,
}

impl ComparePanel {
    pub async fn load(
        workspace: WeakEntity<Workspace>,
        mut cx: AsyncWindowContext,
    ) -> Result<Entity<Self>> {
        workspace.update_in(&mut cx, |workspace, window, cx| {
            let project = workspace.project().clone();
            let weak_workspace = workspace.weak_handle();
            cx.new(|cx| Self::new(project, weak_workspace, window, cx))
        })
    }

    fn new(
        project: Entity<Project>,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let file_tree = cx.new(DiffFileTree::new);
        let tree_subscription = cx.subscribe_in(
            &file_tree,
            window,
            |this, _, event: &DiffFileTreeEvent, window, cx| match event {
                DiffFileTreeEvent::OpenEntry { repo_path, target } => {
                    this.open_entry(repo_path.clone(), *target, window, cx);
                }
            },
        );

        Self {
            fs: project.read(cx).fs().clone(),
            project,
            workspace,
            diff_buffer_list: None,
            base_ref: None,
            file_tree,
            focus_handle: cx.focus_handle(),
            _tree_subscription: tree_subscription,
            _diff_subscription: None,
        }
    }

    fn repository(&self, cx: &App) -> Option<Entity<Repository>> {
        self.project.read(cx).active_repository(cx)
    }

    fn set_base(&mut self, base_ref: SharedString, cx: &mut Context<Self>) {
        let git_store = self.project.read(cx).git_store().clone();
        let repository = self.repository(cx);
        let diff_buffer_list = cx.new(|cx| {
            DiffBufferList::new(
                DiffBase::Merge {
                    base_ref: base_ref.clone(),
                },
                git_store,
                repository,
                cx,
            )
        });
        self._diff_subscription = Some(cx.subscribe(
            &diff_buffer_list,
            |this, _, event: &BranchDiffEvent, cx| match event {
                BranchDiffEvent::FileListChanged | BranchDiffEvent::DiffBaseChanged => {
                    this.refresh_entries(cx)
                }
            },
        ));
        self.diff_buffer_list = Some(diff_buffer_list);
        self.base_ref = Some(base_ref);
        self.refresh_entries(cx);
        cx.notify();
    }

    fn refresh_entries(&mut self, cx: &mut Context<Self>) {
        let Some(diff_buffer_list) = self.diff_buffer_list.as_ref() else {
            return;
        };
        let entries = diff_buffer_list
            .read(cx)
            .statuses_by_path()
            .map(|statuses| {
                statuses
                    .iter()
                    .map(|status| DiffTreeEntry {
                        repo_path: status.repo_path.clone(),
                        status: status.status,
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        self.file_tree
            .update(cx, |file_tree, cx| file_tree.set_entries(entries, cx));
        cx.notify();
    }

    fn choose_base(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(repository) = self.repository(cx) else {
            return;
        };
        let branches = repository.update(cx, |repository, _| repository.branches());
        let commits = repository.update(cx, |repository, _| repository.log_commits(0, Some(500)));
        cx.spawn_in(window, async move |this, cx| {
            let branches = branches.await?.map(|scan| scan.branches).unwrap_or_default();
            let commits = commits.await??;

            let mut options = Vec::with_capacity(branches.len() + commits.len());
            let mut refs: Vec<SharedString> = Vec::with_capacity(branches.len() + commits.len());
            for branch in &branches {
                if branch.is_head {
                    continue;
                }
                options.push(SharedString::from(format!("branch: {}", branch.name())));
                refs.push(branch.name().to_owned().into());
            }
            for commit in &commits {
                let short_sha = commit.sha.get(0..8).unwrap_or(commit.sha.as_ref());
                options.push(SharedString::from(format!(
                    "{short_sha} {} — {}",
                    commit.subject, commit.author_name
                )));
                refs.push(commit.sha.clone());
            }
            anyhow::ensure!(!options.is_empty(), "No branches or commits found");

            let workspace = this.read_with(cx, |this, _| this.workspace.clone())?;
            let selection = cx
                .update(|window, cx| {
                    crate::picker_prompt::prompt(
                        "Compare working tree against",
                        options,
                        workspace,
                        window,
                        cx,
                    )
                })?
                .await;
            let Some(base_ref) = selection.and_then(|index| refs.get(index).cloned()) else {
                return anyhow::Ok(());
            };
            this.update(cx, |this, cx| this.set_base(base_ref, cx))?;
            anyhow::Ok(())
        })
        .detach_and_log_err(cx);
    }

    fn open_entry(
        &mut self,
        repo_path: RepoPath,
        target: OpenTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(base_ref), Some(project_path)) = (
            self.base_ref.clone(),
            self.project_path_for(&repo_path, cx),
        ) else {
            return;
        };

        match target {
            OpenTarget::File => {
                self.workspace
                    .update(cx, |workspace, cx| {
                        workspace
                            .open_path(project_path, None, true, window, cx)
                            .detach_and_log_err(cx);
                    })
                    .ok();
            }
            OpenTarget::SingleBuffer => {
                let Some(repository) = self.repository(cx) else {
                    return;
                };
                let base_oid = self
                    .diff_buffer_list
                    .as_ref()
                    .and_then(|list| list.read(cx).base_oid_for_path(&repo_path))
                    .flatten();
                crate::solo_diff_view::SoloDiffView::open_or_focus_with_base(
                    repo_path,
                    repository,
                    self.workspace.clone(),
                    base_ref,
                    base_oid,
                    window,
                    cx,
                )
                .detach_and_log_err(cx);
            }
            OpenTarget::Default | OpenTarget::MultiBuffer => {
                let Some(repository) = self.repository(cx) else {
                    return;
                };
                let project = self.project.clone();
                let diff_buffer_list = self.diff_buffer_list.clone();
                self.workspace
                    .update(cx, |workspace, cx| {
                        BranchDiff::deploy_branch_diff_with_base_ref(
                            workspace,
                            project,
                            repository,
                            base_ref,
                            diff_buffer_list,
                            window,
                            cx,
                        );
                        if let Some(branch_diff) = workspace.active_item_as::<BranchDiff>(cx) {
                            branch_diff.update(cx, |branch_diff, cx| {
                                branch_diff.move_to_project_path(&project_path, window, cx);
                            });
                        }
                    })
                    .ok();
            }
        }
    }

    fn project_path_for(&self, repo_path: &RepoPath, cx: &App) -> Option<ProjectPath> {
        self.repository(cx)?
            .read(cx)
            .repo_path_to_project_path(repo_path, cx)
    }
}

impl EventEmitter<PanelEvent> for ComparePanel {}

impl Focusable for ComparePanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Panel for ComparePanel {
    fn persistent_name() -> &'static str {
        "ComparePanel"
    }

    fn panel_key() -> &'static str {
        "ComparePanel"
    }

    fn position(&self, _: &Window, cx: &App) -> DockPosition {
        ComparePanelSettings::get_global(cx).dock
    }

    fn position_is_valid(&self, position: DockPosition) -> bool {
        matches!(position, DockPosition::Left | DockPosition::Right)
    }

    fn set_position(&mut self, position: DockPosition, _: &mut Window, cx: &mut Context<Self>) {
        settings::update_settings_file(self.fs.clone(), cx, move |settings, _| {
            settings.compare_panel.get_or_insert_default().dock = Some(position.into())
        });
    }

    fn default_size(&self, _: &Window, cx: &App) -> Pixels {
        ComparePanelSettings::get_global(cx).default_width
    }

    fn icon(&self, _: &Window, _: &App) -> Option<ui::IconName> {
        Some(ui::IconName::Diff)
    }

    fn icon_tooltip(&self, _: &Window, _: &App) -> Option<&'static str> {
        Some("Compare Panel")
    }

    fn toggle_action(&self) -> Box<dyn gpui::Action> {
        Box::new(ToggleFocus)
    }

    fn activation_priority(&self) -> u32 {
        4
    }
}

impl Render for ComparePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header: SharedString = match &self.base_ref {
            Some(base_ref) => {
                let short = short_ref(base_ref);
                let count = self.file_tree.read(cx).len();
                if count == 1 {
                    format!("1 file vs {short}").into()
                } else {
                    format!("{count} files vs {short}").into()
                }
            }
            None => "No comparison target".into(),
        };

        v_flex()
            .key_context("ComparePanel")
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(cx.theme().colors().panel_background)
            .child(
                h_flex()
                    .px_2()
                    .py_1()
                    .gap_2()
                    .justify_between()
                    .border_b_1()
                    .border_color(cx.theme().colors().border)
                    .child(
                        Label::new(header)
                            .size(LabelSize::Small)
                            .color(Color::Muted)
                            .truncate(),
                    )
                    .child(
                        Button::new("choose-compare-base", "Change")
                            .label_size(LabelSize::Small)
                            .tooltip(Tooltip::text("Choose a branch or commit to compare against"))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.choose_base(window, cx)),
                            ),
                    ),
            )
            .map(|el| {
                if self.diff_buffer_list.is_some() {
                    el.child(self.file_tree.clone())
                } else {
                    el.child(
                        v_flex().size_full().items_center().justify_center().child(
                            Button::new("pick-compare-base", "Choose a branch or commit…").on_click(
                                cx.listener(|this, _, window, cx| this.choose_base(window, cx)),
                            ),
                        ),
                    )
                }
            })
    }
}

pub(crate) fn short_ref(base_ref: &str) -> &str {
    if base_ref.len() == 40 && base_ref.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        &base_ref[..8]
    } else {
        base_ref
    }
}
