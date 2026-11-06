use workspace::Workspace;

mod panel;
mod runner;
mod status_item;

pub use panel::AospPanel;
pub use runner::{CommandOutput, run_aosp_ide_gen};
pub use status_item::AospStatusItem;

gpui::actions!(
    aosp_ui,
    [
        /// Toggles focus on the AOSP panel.
        ToggleFocus
    ]
);

pub fn init(cx: &mut gpui::App) {
    cx.observe_new(|workspace: &mut Workspace, _, _| {
        workspace.register_action(|workspace, _: &ToggleFocus, window, cx| {
            workspace.toggle_panel_focus::<AospPanel>(window, cx);
        });
    })
    .detach();
}
