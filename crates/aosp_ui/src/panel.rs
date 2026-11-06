use gpui::{
    Action, App, AsyncWindowContext, ClipboardItem, Entity, EventEmitter, FocusHandle, Focusable,
    Pixels, ScrollHandle, SharedString, Task, WeakEntity, Window,
};
use project::Project;
use ui::{WithScrollbar, prelude::*};
use util::ResultExt;
use workspace::Workspace;
use workspace::dock::{DockPosition, Panel, PanelEvent};

use crate::ToggleFocus;
use crate::runner::{BINARY_NAME, BINARY_PATH, CommandOutput, run_aosp_ide_gen};

pub struct AospPanel {
    project: Entity<Project>,
    focus_handle: FocusHandle,
    position: DockPosition,
    state: DoctorState,
    scroll_handle: ScrollHandle,
}

enum DoctorState {
    Idle,
    Running { _task: Task<()> },
    Done(CommandOutput),
    Failed(SharedString),
}

impl AospPanel {
    pub async fn load(
        workspace: WeakEntity<Workspace>,
        mut cx: AsyncWindowContext,
    ) -> anyhow::Result<Entity<Self>> {
        workspace.update_in(&mut cx, |workspace, _, cx| {
            cx.new(|cx| Self {
                project: workspace.project().clone(),
                focus_handle: cx.focus_handle(),
                position: DockPosition::Right,
                state: DoctorState::Idle,
                scroll_handle: ScrollHandle::new(),
            })
        })
    }

    fn run_doctor(&mut self, cx: &mut Context<Self>) {
        let doctor = run_aosp_ide_gen(&self.project, &["doctor".into()], cx);
        self.state = DoctorState::Running {
            _task: cx.spawn(async move |this, cx| {
                let state = match doctor.await {
                    Ok(output) => DoctorState::Done(output),
                    Err(error) => DoctorState::Failed(format!("{error:#}").into()),
                };
                this.update(cx, |panel, cx| {
                    panel.state = state;
                    cx.notify();
                })
                .log_err();
            }),
        };
        cx.notify();
    }
}

/// Text lines shown for a finished command, in order:
/// "exit code: N" or "terminated by signal", stdout, stderr,
/// then "expected at $HOME/bin/aosp-ide-gen" only when the code is 126 or 127.
fn output_lines(output: &CommandOutput) -> Vec<String> {
    let status = match output.exit_code {
        Some(exit_code) => format!("exit code: {exit_code}"),
        None => "terminated by signal".to_string(),
    };
    let mut lines = vec![status, output.stdout.clone(), output.stderr.clone()];
    // bash 3.2 (macOS /bin/sh) exits 126 when `exec` fails on a path, POSIX shells exit 127.
    if matches!(output.exit_code, Some(126) | Some(127)) {
        lines.push(format!("expected at {BINARY_PATH}"));
    }
    lines
}

/// Plain text for the "Copy output" button: `output_lines` trimmed of trailing
/// newlines, empty entries dropped, joined with "\n".
fn output_text(output: &CommandOutput) -> String {
    let lines = output_lines(output);
    lines
        .iter()
        .map(|line| line.trim_end_matches('\n'))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

impl Render for AospPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (lines, copy_text): (Vec<SharedString>, Option<SharedString>) = match &self.state {
            DoctorState::Idle => (vec![], None),
            DoctorState::Running { .. } => {
                (vec![format!("Running {BINARY_NAME} doctor…").into()], None)
            }
            DoctorState::Done(output) => {
                let text = output_text(output).into();
                (
                    output_lines(output)
                        .into_iter()
                        .map(SharedString::from)
                        .collect(),
                    Some(text),
                )
            }
            DoctorState::Failed(message) => (vec![message.clone()], Some(message.clone())),
        };

        v_flex()
            .id("aosp-panel")
            .size_full()
            .p_2()
            .gap_1()
            .track_focus(&self.focus_handle(cx))
            .child(
                Button::new("aosp-doctor", "Doctor")
                    .disabled(matches!(self.state, DoctorState::Running { .. }))
                    .on_click(cx.listener(|panel, _, _window, cx| panel.run_doctor(cx))),
            )
            .when_some(copy_text, |this, text| {
                this.child(Button::new("aosp-copy-output", "Copy output").on_click(
                    move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()))
                    },
                ))
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .id("aosp-output")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll_handle)
                            .children(lines.into_iter().map(Label::new)),
                    )
                    .vertical_scrollbar_for(&self.scroll_handle, window, cx),
            )
    }
}

impl Focusable for AospPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<PanelEvent> for AospPanel {}

impl Panel for AospPanel {
    fn persistent_name() -> &'static str {
        "AospPanel"
    }

    fn panel_key() -> &'static str {
        "AospPanel"
    }

    fn position(&self, _window: &Window, _cx: &App) -> DockPosition {
        self.position
    }

    fn position_is_valid(&self, position: DockPosition) -> bool {
        matches!(position, DockPosition::Left | DockPosition::Right)
    }

    fn set_position(
        &mut self,
        position: DockPosition,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        self.position = position
    }

    fn default_size(&self, _window: &Window, _cx: &App) -> Pixels {
        px(360.)
    }

    fn icon(&self, _window: &Window, _cx: &App) -> Option<IconName> {
        None
    }

    fn icon_tooltip(&self, _window: &Window, _cx: &App) -> Option<&'static str> {
        None
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(ToggleFocus)
    }

    fn activation_priority(&self) -> u32 {
        8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_list_exit_code_stdout_and_stderr_when_command_finished() {
        assert_eq!(
            output_lines(&CommandOutput {
                exit_code: Some(0),
                stdout: "ok\n".to_string(),
                stderr: "".to_string(),
            }),
            Vec::from([
                "exit code: 0".to_string(),
                "ok\n".to_string(),
                "".to_string(),
            ]),
        );
    }

    #[test]
    fn should_hint_expected_binary_path_when_exit_code_is_127() {
        assert_eq!(
            output_lines(&CommandOutput {
                exit_code: Some(127),
                stdout: "".to_string(),
                stderr: "sh: not found\n".to_string(),
            })
            .last(),
            Some("expected at $HOME/bin/aosp-ide-gen".to_string()).as_ref(),
        );
    }

    #[test]
    fn should_report_signal_when_exit_code_is_missing() {
        assert_eq!(
            output_lines(&CommandOutput {
                exit_code: None,
                stdout: "".to_string(),
                stderr: "".to_string(),
            })
            .first(),
            Some("terminated by signal".to_string()).as_ref(),
        );
    }

    #[test]
    fn should_hint_expected_binary_path_when_exit_code_is_126() {
        assert_eq!(
            output_lines(&CommandOutput {
                exit_code: Some(126),
                stdout: "".to_string(),
                stderr: "sh: line 0: exec: not found\n".to_string(),
            })
            .last(),
            Some("expected at $HOME/bin/aosp-ide-gen".to_string()).as_ref(),
        );
    }

    #[test]
    fn should_join_output_lines_without_blank_lines_when_copying() {
        assert_eq!(
            output_text(&CommandOutput {
                exit_code: Some(1),
                stdout: "FAIL a\nOK   b\n".to_string(),
                stderr: "".to_string(),
            }),
            "exit code: 1\nFAIL a\nOK   b".to_string(),
        );
    }

    #[test]
    fn should_keep_stderr_and_hint_when_copying_missing_binary_output() {
        assert_eq!(
            output_text(&CommandOutput {
                exit_code: Some(126),
                stdout: "".to_string(),
                stderr: "sh: exec: not found\n".to_string(),
            }),
            "exit code: 126\nsh: exec: not found\nexpected at $HOME/bin/aosp-ide-gen".to_string(),
        );
    }
}
