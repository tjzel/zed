use std::any::{Any, TypeId};
use std::sync::Arc;

use anyhow::Result;
use buffer_diff::BufferDiff;
use editor::{Editor, EditorEvent, EditorSettings, MultiBuffer, SplittableEditor};
use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Render,
    Task, Window,
};
use language::Buffer;
use project::{Project, ProjectPath};
use settings::Settings;
use ui::{Color, Icon, IconName, Label, LabelCommon as _, SharedString, prelude::*};
use workspace::{
    Item, ItemNavHistory, Workspace,
    item::{ItemEvent, SaveOptions, TabContentParams},
    searchable::SearchableItemHandle,
};

pub struct CompareFileView {
    editor: Entity<SplittableEditor>,
    project_path: ProjectPath,
    base_ref: SharedString,
    title: SharedString,
    tooltip: SharedString,
}

impl CompareFileView {
    pub fn new(
        buffer: Entity<Buffer>,
        diff: Entity<BufferDiff>,
        project_path: ProjectPath,
        base_ref: SharedString,
        project: Entity<Project>,
        workspace: Entity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let multibuffer = cx.new(|cx| {
            let mut multibuffer = MultiBuffer::singleton(buffer.clone(), cx);
            multibuffer.add_diff(diff.clone(), cx);
            multibuffer
        });
        let editor = cx.new(|cx| {
            let splittable = SplittableEditor::new(
                EditorSettings::get_global(cx).diff_view_style,
                multibuffer,
                project,
                workspace,
                window,
                cx,
            );
            splittable.set_render_diff_hunk_controls(
                Arc::new(|_, _, _, _, _, _, _, _| gpui::Empty.into_any_element()),
                cx,
            );
            splittable.rhs_editor().update(cx, |editor, cx| {
                editor.start_temporary_diff_override();
                editor.disable_diagnostics(cx);
                editor.set_expand_all_diff_hunks(cx);
            });
            splittable
        });

        cx.subscribe(&editor, |_, _, event: &EditorEvent, cx| {
            cx.emit(event.clone());
        })
        .detach();

        let file_name: SharedString = project_path
            .path
            .file_name()
            .unwrap_or_default()
            .to_string()
            .into();
        let short_base = short_base_ref(&base_ref);
        let title: SharedString = format!("{file_name} ↔ {short_base}").into();
        let tooltip: SharedString =
            format!("{} compared with {}", project_path.path.as_unix_str(), base_ref).into();

        Self {
            editor,
            project_path,
            base_ref,
            title,
            tooltip,
        }
    }

    pub fn project_path(&self) -> &ProjectPath {
        &self.project_path
    }

    pub fn base_ref(&self) -> &SharedString {
        &self.base_ref
    }
}

fn short_base_ref(base_ref: &str) -> &str {
    if base_ref.len() == 40 && base_ref.bytes().all(|b| b.is_ascii_hexdigit()) {
        &base_ref[..8]
    } else {
        base_ref
    }
}

impl EventEmitter<EditorEvent> for CompareFileView {}

impl Focusable for CompareFileView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.editor.focus_handle(cx)
    }
}

impl Item for CompareFileView {
    type Event = EditorEvent;

    fn tab_icon(&self, _window: &Window, _cx: &App) -> Option<Icon> {
        Some(Icon::new(IconName::Diff).color(Color::Muted))
    }

    fn tab_content(&self, params: TabContentParams, _window: &Window, cx: &App) -> AnyElement {
        Label::new(self.tab_content_text(params.detail.unwrap_or_default(), cx))
            .color(if params.selected {
                Color::Default
            } else {
                Color::Muted
            })
            .into_any_element()
    }

    fn tab_content_text(&self, _detail: usize, _: &App) -> SharedString {
        self.title.clone()
    }

    fn tab_tooltip_text(&self, _: &App) -> Option<SharedString> {
        Some(self.tooltip.clone())
    }

    fn to_item_events(event: &EditorEvent, f: &mut dyn FnMut(ItemEvent)) {
        Editor::to_item_events(event, f)
    }

    fn telemetry_event_text(&self) -> Option<&'static str> {
        Some("Compare File View Opened")
    }

    fn deactivated(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editor
            .update(cx, |editor, cx| editor.deactivated(window, cx));
    }

    fn act_as_type<'a>(
        &'a self,
        type_id: TypeId,
        self_handle: &'a Entity<Self>,
        cx: &'a App,
    ) -> Option<gpui::AnyEntity> {
        if type_id == TypeId::of::<Self>() {
            Some(self_handle.clone().into())
        } else if type_id == TypeId::of::<SplittableEditor>() {
            Some(self.editor.clone().into())
        } else if type_id == TypeId::of::<Editor>() {
            Some(self.editor.read(cx).rhs_editor().clone().into())
        } else {
            None
        }
    }

    fn as_searchable(&self, _: &Entity<Self>, _: &App) -> Option<Box<dyn SearchableItemHandle>> {
        Some(Box::new(self.editor.clone()))
    }

    fn for_each_project_item(
        &self,
        cx: &App,
        f: &mut dyn FnMut(gpui::EntityId, &dyn project::ProjectItem),
    ) {
        self.editor.read(cx).for_each_project_item(cx, f)
    }

    fn set_nav_history(
        &mut self,
        nav_history: ItemNavHistory,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rhs = self.editor.read(cx).rhs_editor().clone();
        rhs.update(cx, |editor, _| {
            editor.set_nav_history(Some(nav_history));
        });
    }

    fn navigate(
        &mut self,
        data: Arc<dyn Any + Send>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.editor
            .update(cx, |editor, cx| editor.navigate(data, window, cx))
    }

    fn added_to_workspace(
        &mut self,
        workspace: &mut Workspace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor.update(cx, |editor, cx| {
            editor.added_to_workspace(workspace, window, cx)
        });
    }

    fn is_dirty(&self, cx: &App) -> bool {
        self.editor.read(cx).is_dirty(cx)
    }

    fn can_save(&self, cx: &App) -> bool {
        self.editor.read(cx).can_save(cx)
    }

    fn save(
        &mut self,
        options: SaveOptions,
        project: Entity<Project>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        self.editor
            .update(cx, |editor, cx| editor.save(options, project, window, cx))
    }
}

impl Render for CompareFileView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.editor.clone())
    }
}
