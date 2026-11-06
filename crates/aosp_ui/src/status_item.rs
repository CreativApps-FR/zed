use gpui::WeakEntity;
use ui::{Tooltip, prelude::*};
use util::ResultExt;
use workspace::{HideStatusItem, StatusItemView, Workspace, item::ItemHandle};

use crate::AospPanel;
use crate::ToggleFocus;

pub struct AospStatusItem {
    workspace: WeakEntity<Workspace>,
}

impl AospStatusItem {
    pub fn new(workspace: &Workspace) -> Self {
        AospStatusItem {
            workspace: workspace.weak_handle(),
        }
    }

    /// Click handler of the `AOSP` label.
    fn on_label_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.workspace
            .update(cx, |workspace, cx| toggle_panel(workspace, window, cx))
            .log_err();
    }
}

/// Same rule as Zed's dock buttons: if the AOSP panel is the visible panel of
/// an open dock, close that dock; otherwise open the dock and focus the panel.
fn toggle_panel(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    let aosp_is_shown = workspace.all_docks().iter().any(|dock| {
        dock.read(cx)
            .visible_panel()
            .is_some_and(|panel| panel.to_any().downcast::<AospPanel>().is_ok())
    });
    if aosp_is_shown {
        workspace.close_panel::<AospPanel>(window, cx);
    } else {
        workspace.focus_panel::<AospPanel>(window, cx);
    }
}

impl Render for AospStatusItem {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().child(
            Button::new("aosp-status", "AOSP")
                .label_size(LabelSize::Small)
                .tab_index(0isize)
                .on_click(cx.listener(|this, _, window, cx| this.on_label_click(window, cx)))
                .tooltip(|_window, cx| Tooltip::for_action("Toggle AOSP Panel", &ToggleFocus, cx)),
        )
    }
}

impl StatusItemView for AospStatusItem {
    fn set_active_pane_item(
        &mut self,
        _active_pane_item: Option<&dyn ItemHandle>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        // The label does not depend on the active pane item.
    }

    fn hide_setting(&self, _cx: &App) -> Option<HideStatusItem> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fs::FakeFs;
    use gpui::TestAppContext;
    use project::Project;
    use settings::SettingsStore;

    #[gpui::test]
    async fn should_close_panel_when_label_clicked_while_panel_is_shown(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let settings_store = SettingsStore::test(cx);
            cx.set_global(settings_store);
            cx.set_global(db::AppDatabase::test_new());
            theme_settings::init(theme::LoadThemes::JustBase, cx);
        });

        let fs = FakeFs::new(cx.executor());
        let project = Project::test(fs, [], cx).await;
        let (workspace, cx) =
            cx.add_window_view(|window, cx| Workspace::test_new(project, window, cx));

        let panel = workspace
            .update_in(cx, |workspace, window, cx| {
                let weak_workspace = workspace.weak_handle();
                cx.spawn_in(window, async move |_, cx| {
                    AospPanel::load(weak_workspace, cx.clone()).await
                })
            })
            .await
            .unwrap();

        workspace.update_in(cx, |workspace, window, cx| {
            workspace.add_panel(panel, window, cx);
        });
        cx.run_until_parked();

        // First click: the dock starts closed, so clicking opens it and focuses the panel.
        workspace.update_in(cx, |workspace, window, cx| {
            toggle_panel(workspace, window, cx);
            assert!(
                workspace.right_dock().read(cx).is_open(),
                "Dock should be open after the first click",
            );
        });

        // Second click: the AOSP panel is the visible panel of an open dock, so the dock must close.
        workspace.update_in(cx, |workspace, window, cx| {
            toggle_panel(workspace, window, cx);
            assert!(
                !workspace.right_dock().read(cx).is_open(),
                "Dock should close when the AOSP label is clicked while the panel is shown",
            );
        });
    }

    #[gpui::test]
    async fn should_toggle_panel_when_label_click_handler_runs_inside_window(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            let settings_store = SettingsStore::test(cx);
            cx.set_global(settings_store);
            cx.set_global(db::AppDatabase::test_new());
            theme_settings::init(theme::LoadThemes::JustBase, cx);
        });

        let fs = FakeFs::new(cx.executor());
        let project = Project::test(fs, [], cx).await;
        let (workspace, cx) =
            cx.add_window_view(|window, cx| Workspace::test_new(project, window, cx));

        let panel = workspace
            .update_in(cx, |workspace, window, cx| {
                let weak_workspace = workspace.weak_handle();
                cx.spawn_in(window, async move |_, cx| {
                    AospPanel::load(weak_workspace, cx.clone()).await
                })
            })
            .await
            .unwrap();

        workspace.update_in(cx, |workspace, window, cx| {
            workspace.add_panel(panel, window, cx);
        });
        cx.run_until_parked();

        let status_item = workspace.update_in(cx, |workspace, _window, cx| {
            cx.new(|_| AospStatusItem::new(workspace))
        });

        // First click: the dock starts closed, so clicking opens it and focuses the panel.
        status_item.update_in(cx, |item, window, cx| item.on_label_click(window, cx));
        assert!(
            workspace.read_with(cx, |workspace, cx| workspace
                .right_dock()
                .read(cx)
                .is_open()),
            "Dock should be open after the first click",
        );

        // Second click: the AOSP panel is the visible panel of an open dock, so the dock must close.
        status_item.update_in(cx, |item, window, cx| item.on_label_click(window, cx));
        assert!(
            !workspace.read_with(cx, |workspace, cx| workspace
                .right_dock()
                .read(cx)
                .is_open()),
            "Dock should close when the AOSP label is clicked while the panel is shown",
        );
    }
}
