use anyhow::{Context as _, Result, anyhow};
use collections::HashMap;
use gpui::{App, AppContext as _, Entity, Task};
use project::Project;
use remote::{CommandTemplate, Interactive};
use std::path::Path;

pub(crate) const BINARY_NAME: &str = "aosp-ide-gen";
pub(crate) const BINARY_PATH: &str = "$HOME/bin/aosp-ide-gen";
const HOME_BIN_SCRIPT: &str = "exec \"$HOME/bin/aosp-ide-gen\" \"$@\"";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandOutput {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

pub(crate) fn wrapped_args(args: &[impl AsRef<str>]) -> Vec<String> {
    let mut wrapped: Vec<String> = Vec::from([
        "-c".to_string(),
        HOME_BIN_SCRIPT.to_string(),
        BINARY_NAME.to_string(),
    ]);
    wrapped.extend(args.iter().map(|argument| argument.as_ref().to_string()));
    wrapped
}

/// Runs `$HOME/bin/aosp-ide-gen <args>` in the first visible worktree,
/// through the project's remote connection when there is one.
pub fn run_aosp_ide_gen(
    project: &Entity<Project>,
    args: &[String],
    cx: &mut App,
) -> Task<Result<CommandOutput>> {
    let summary = args.join(" ");
    let directory = project
        .read(cx)
        .visible_worktrees(cx)
        .next()
        .map(|worktree| worktree.read(cx).abs_path().display().to_string());
    let Some(directory) = directory else {
        return Task::ready(
            Err(anyhow!("Open an AOSP folder first"))
                .with_context(|| format!("Failed to run {BINARY_NAME} {summary}")),
        );
    };

    let command_args = wrapped_args(args);
    let remote_client = project.read(cx).remote_client();

    if let Some(remote_client) = remote_client {
        let template = remote_client
            .read(cx)
            .build_command(
                Some("sh".to_string()),
                &command_args,
                &HashMap::default(),
                Some(directory),
                None,
                Interactive::No,
            )
            .context("building SSH command");
        return match template {
            Err(error) => Task::ready(
                Err(error).with_context(|| format!("Failed to run {BINARY_NAME} {summary}")),
            ),
            Ok(template) => {
                let CommandTemplate {
                    program,
                    args: template_args,
                    env,
                } = template;
                cx.background_spawn(async move {
                    run_command(program, &template_args, &env, None)
                        .await
                        .with_context(|| format!("Failed to run {BINARY_NAME} {summary}"))
                })
            }
        };
    }

    cx.background_spawn(async move {
        let env: HashMap<String, String> = HashMap::default();
        run_command("sh", &command_args, &env, Some(Path::new(&directory)))
            .await
            .with_context(|| format!("Failed to run {BINARY_NAME} {summary}"))
    })
}

/// Spawns `program` as a local process and collects its output.
pub(crate) async fn run_command(
    program: impl AsRef<std::ffi::OsStr>,
    args: &[String],
    env: &HashMap<String, String>,
    directory: Option<&Path>,
) -> Result<CommandOutput> {
    let mut command = util::command::new_command(program);
    command.args(args).envs(env).kill_on_drop(true);
    if let Some(directory) = directory {
        command.current_dir(directory);
    }
    let output = command.output().await.context("spawning process")?;
    Ok(CommandOutput {
        exit_code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fs::FakeFs;
    use gpui::TestAppContext;
    use project::Project;
    use settings::SettingsStore;
    #[cfg(unix)]
    use std::fs::{canonicalize, create_dir, set_permissions, write};
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    #[cfg(unix)]
    use tempfile::TempDir;

    #[test]
    fn should_wrap_arguments_for_home_bin_binary() {
        assert_eq!(
            wrapped_args(&["doctor", "a b", "it's"]),
            Vec::from([
                "-c".to_string(),
                HOME_BIN_SCRIPT.to_string(),
                BINARY_NAME.to_string(),
                "doctor".to_string(),
                "a b".to_string(),
                "it's".to_string(),
            ]),
        );
    }

    #[test]
    fn should_execute_the_hinted_path_when_wrapping_arguments() {
        assert!(HOME_BIN_SCRIPT.contains(BINARY_PATH));
    }

    #[cfg(unix)]
    #[test]
    fn should_return_stdout_stderr_and_exit_code_when_command_runs_locally() {
        smol::block_on(async {
            let work = TempDir::new().unwrap();
            let home = TempDir::new().unwrap();
            let binary_dir = home.path().join("bin");
            create_dir(&binary_dir).unwrap();
            let binary = binary_dir.join("aosp-ide-gen");
            write(
                &binary,
                "#!/bin/sh\necho \"PWD=$PWD\"\nfor argument in \"$@\"; do echo \"$argument\"; done\necho err >&2\nexit 3\n",
            ).unwrap();
            set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();

            let env: HashMap<String, String> =
                HashMap::from_iter([("HOME".to_string(), home.path().display().to_string())]);
            let work_dir = work.path();
            let output = run_command(
                "sh",
                &wrapped_args(&["doctor", "a b"]),
                &env,
                Some(&work_dir),
            )
            .await
            .unwrap();

            assert_eq!(
                output,
                CommandOutput {
                    exit_code: Some(3),
                    stdout: format!(
                        "PWD={}\ndoctor\na b\n",
                        canonicalize(work_dir).unwrap().display(),
                    ),
                    stderr: "err\n".to_string(),
                },
            );
        });
    }

    // macOS ships bash 3.2 as /bin/sh, which exits 126 when `exec` fails on a path;
    // POSIX (and every Linux shell) specifies 127 for a not-found command.
    #[cfg(all(unix, target_os = "macos"))]
    const EXPECTED_MISSING_BINARY_EXIT_CODE: i32 = 126;
    #[cfg(all(unix, not(target_os = "macos")))]
    const EXPECTED_MISSING_BINARY_EXIT_CODE: i32 = 127;

    #[cfg(unix)]
    #[test]
    fn should_return_exit_code_127_when_binary_is_missing() {
        smol::block_on(async {
            let home = TempDir::new().unwrap();
            let env: HashMap<String, String> =
                HashMap::from_iter([("HOME".to_string(), home.path().display().to_string())]);
            let output = run_command("sh", &wrapped_args(&["doctor"]), &env, None)
                .await
                .unwrap();

            assert_eq!(output.exit_code, Some(EXPECTED_MISSING_BINARY_EXIT_CODE));
            assert!(!output.stderr.is_empty());
        });
    }

    #[gpui::test]
    async fn should_fail_with_open_folder_message_when_project_has_no_worktree(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            let settings_store = SettingsStore::test(cx);
            cx.set_global(settings_store);
        });

        let fs = FakeFs::new(cx.executor());
        let project = Project::test(fs, [], cx).await;
        let failure = cx
            .update(|cx| run_aosp_ide_gen(&project, &["doctor".to_string()], cx))
            .await;
        let error = failure.unwrap_err();
        assert!(format!("{:#}", error).contains("Open an AOSP folder first"));
    }

    #[test]
    fn should_fail_with_spawn_context_when_program_does_not_exist() {
        smol::block_on(async {
            let env: HashMap<String, String> = HashMap::default();
            let error = run_command("aosp-ui-no-such-program", &[], &env, None)
                .await
                .unwrap_err();
            assert!(format!("{:#}", error).contains("spawning process"));
        });
    }
}
