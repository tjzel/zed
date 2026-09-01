use anyhow::Result;
use gpui::{
    App, AsyncWindowContext, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement,
    Render, SharedString, Subscription, WeakEntity, Window, actions,
};
use multi_buffer::PathKey;
use project::Project;
use project::git_store::branch_diff::{BranchDiff, BranchDiffEvent, DiffBase};
use ui::{Button, ButtonCommon as _, Clickable as _, Tooltip, prelude::*};
use workspace::Workspace;
use workspace::dock::{DockPosition, Panel, PanelEvent};

use crate::compare_file_view::open_single_file_diff;
use crate::diff_file_tree::{DiffFileTree, DiffFileTreeEvent, DiffTreeEntry};
use crate::project_diff::pick_compare_base;

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

pub struct ComparePanel {
    project: Entity<Project>,
    workspace: WeakEntity<Workspace>,
    branch_diff: Option<Entity<BranchDiff>>,
    base_ref: Option<SharedString>,
    file_tree: Entity<DiffFileTree>,
    position: DockPosition,
    focus_handle: FocusHandle,
    _tree_subscription: Subscription,
    _branch_diff_subscription: Option<Subscription>,
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
                DiffFileTreeEvent::OpenEntry { repo_path, .. } => {
                    this.open_file(repo_path.clone(), window, cx);
                }
            },
        );

        Self {
            project,
            workspace,
            branch_diff: None,
            base_ref: None,
            file_tree,
            position: DockPosition::Right,
            focus_handle: cx.focus_handle(),
            _tree_subscription: tree_subscription,
            _branch_diff_subscription: None,
        }
    }

    fn open_file(
        &mut self,
        repo_path: git::repository::RepoPath,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(branch_diff), Some(base_ref)) =
            (self.branch_diff.clone(), self.base_ref.clone())
        else {
            return;
        };
        open_single_file_diff(
            branch_diff,
            repo_path,
            base_ref,
            self.project.clone(),
            self.workspace.clone(),
            window,
            cx,
        );
    }

    fn choose_base(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(repo) = self
            .project
            .read(cx)
            .git_store()
            .read(cx)
            .active_repository()
        else {
            return;
        };
        let picker = pick_compare_base(repo, self.workspace.clone(), window, cx);
        cx.spawn_in(window, async move |this, cx| {
            let Some(base_ref) = picker.await? else {
                return anyhow::Ok(());
            };
            this.update_in(cx, |this, window, cx| this.set_base(base_ref, window, cx))?;
            anyhow::Ok(())
        })
        .detach_and_log_err(cx);
    }

    fn set_base(&mut self, base_ref: SharedString, window: &mut Window, cx: &mut Context<Self>) {
        let branch_diff = cx.new(|cx| {
            BranchDiff::new(
                DiffBase::Merge {
                    base_ref: base_ref.clone(),
                },
                self.project.clone(),
                window,
                cx,
            )
        });
        self._branch_diff_subscription = Some(cx.subscribe(
            &branch_diff,
            |this, _, event: &BranchDiffEvent, cx| match event {
                BranchDiffEvent::FileListChanged => this.refresh_entries(cx),
            },
        ));
        self.branch_diff = Some(branch_diff);
        self.base_ref = Some(base_ref);
        self.refresh_entries(cx);
        cx.notify();
    }

    fn refresh_entries(&mut self, cx: &mut Context<Self>) {
        let Some(branch_diff) = self.branch_diff.as_ref() else {
            return;
        };
        let entries = branch_diff
            .read(cx)
            .entries(cx)
            .into_iter()
            .map(|(repo_path, status)| DiffTreeEntry {
                path_key: PathKey::with_sort_prefix(0, repo_path.as_ref().clone()),
                repo_path,
                status,
            })
            .collect();
        self.file_tree
            .update(cx, |file_tree, cx| file_tree.set_entries(entries, cx));
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

    fn position(&self, _: &Window, _: &App) -> DockPosition {
        self.position
    }

    fn position_is_valid(&self, position: DockPosition) -> bool {
        matches!(position, DockPosition::Left | DockPosition::Right)
    }

    fn set_position(&mut self, position: DockPosition, _: &mut Window, cx: &mut Context<Self>) {
        self.position = position;
        cx.notify();
    }

    fn default_size(&self, _: &Window, _: &App) -> Pixels {
        px(320.)
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
        let base_label: SharedString = match &self.base_ref {
            Some(base_ref) => {
                let base_ref: &str = base_ref;
                if base_ref.len() == 40 && base_ref.bytes().all(|b| b.is_ascii_hexdigit()) {
                    format!("Comparing against {}", &base_ref[..8]).into()
                } else {
                    format!("Comparing against {base_ref}").into()
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
                        Label::new(base_label)
                            .size(LabelSize::Small)
                            .color(Color::Muted)
                            .truncate(),
                    )
                    .child(
                        Button::new("choose-compare-base", "Change")
                            .label_size(LabelSize::Small)
                            .tooltip(Tooltip::text("Choose a branch or commit to compare against"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.choose_base(window, cx)
                            })),
                    ),
            )
            .map(|el| {
                if self.branch_diff.is_some() {
                    el.child(self.file_tree.clone())
                } else {
                    el.child(
                        v_flex().size_full().items_center().justify_center().child(
                            Button::new("pick-compare-base", "Choose branch or commit…").on_click(
                                cx.listener(|this, _, window, cx| this.choose_base(window, cx)),
                            ),
                        ),
                    )
                }
            })
    }
}
