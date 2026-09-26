//! "Delete Aural": the confirmation rule and the list of what gets removed. The app
//! executes the plan; keeping it pure makes the guard and the scope testable.

use crate::paths::AppPaths;
use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

/// The exact phrase the user must type. Case, spacing and punctuation must match;
/// nothing is trimmed, so a stray space or newline does not confirm.
pub const CONFIRMATION_PHRASE: &str = "YES, DELETE";

pub fn is_confirmed(input: &str) -> bool {
    input == CONFIRMATION_PHRASE
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub program: PathBuf,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletionPlan {
    /// Aural's own data and config roots (models, logs, settings).
    pub remove_dirs: Vec<PathBuf>,
    /// Remove the "start with Windows" registry value.
    pub remove_autostart: bool,
    /// NSIS uninstaller to launch after the app exits, when installed.
    pub run_uninstaller: Option<Command>,
    /// Only directories under these roots may ever be deleted.
    allowed_roots: Vec<PathBuf>,
    /// Folder of the running program; never deleted from here (the uninstaller does).
    install_dir: Option<PathBuf>,
}

/// `install_dir` is the folder of the running executable; the NSIS installer puts
/// `uninstall.exe` there. Dev and portable builds have none, so only data is removed.
pub fn plan(paths: &AppPaths, install_dir: Option<&Path>) -> DeletionPlan {
    let run_uninstaller = install_dir
        .map(|d| d.join("uninstall.exe"))
        .filter(|p| p.is_file())
        .map(|program| Command {
            program,
            args: vec!["/S".into()],
        });
    DeletionPlan {
        remove_dirs: vec![paths.data_dir.clone(), paths.config_dir.clone()],
        remove_autostart: true,
        run_uninstaller,
        allowed_roots: vec![paths.data_dir.clone(), paths.config_dir.clone()],
        install_dir: install_dir.map(Path::to_path_buf),
    }
}

impl DeletionPlan {
    /// Refuses anything that is not one of Aural's own roots, and any root that
    /// contains the installed program, so a wrong plan can never delete other folders.
    pub fn validate(&self) -> Result<()> {
        for dir in &self.remove_dirs {
            if !self.allowed_roots.iter().any(|r| r == dir) {
                bail!("refusing to delete {}: not an Aural folder", dir.display());
            }
            if self
                .install_dir
                .as_ref()
                .is_some_and(|i| i.starts_with(dir))
            {
                bail!(
                    "refusing to delete {}: it contains the installed program",
                    dir.display()
                );
            }
        }
        Ok(())
    }

    /// Command line for `cmd.exe /C`, run after Aural has exited (WebView2 and the
    /// speech worker hold files open until then): wait ~2 s, delete the data and
    /// settings folders, then run the uninstaller silently.
    pub fn after_exit_command(&self) -> Result<String> {
        self.validate()?;
        let quote = |p: &Path| -> Result<String> {
            let s = p.display().to_string();
            // Inside double quotes cmd treats & | < > ^ literally, but not " or %.
            if s.contains(['"', '%', '\r', '\n']) {
                bail!("cannot safely pass {s:?} to the cleanup step");
            }
            Ok(format!("\"{s}\""))
        };
        let mut steps = vec!["ping 127.0.0.1 -n 3 >nul".to_owned()];
        for dir in &self.remove_dirs {
            steps.push(format!("rmdir /s /q {}", quote(dir)?));
        }
        if let Some(u) = &self.run_uninstaller {
            steps.push(format!("{} {}", quote(&u.program)?, u.args.join(" ")));
        }
        Ok(steps.join(" & "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::AppPaths;
    use std::path::Path;

    #[test]
    fn only_the_exact_phrase_confirms() {
        assert!(is_confirmed("YES, DELETE"));
        for wrong in [
            "",
            "yes, delete",
            "YES DELETE",
            "YES,DELETE",
            " YES, DELETE",
            "YES, DELETE ",
            "YES, DELETE.",
            "Yes, Delete",
            "YES, DELETE\n",
            "YES，DELETE",
        ] {
            assert!(!is_confirmed(wrong), "{wrong:?} must not confirm");
        }
    }

    #[test]
    fn plan_covers_data_config_and_autostart_only() {
        let paths = AppPaths::under(Path::new("C:/root"));
        let plan = plan(&paths, None);
        assert_eq!(
            plan.remove_dirs,
            vec![
                Path::new("C:/root/data").to_path_buf(),
                Path::new("C:/root/config").to_path_buf()
            ]
        );
        assert!(plan.remove_autostart);
        assert_eq!(plan.run_uninstaller, None);
    }

    #[test]
    fn plan_runs_the_nsis_uninstaller_silently_when_installed() {
        let paths = AppPaths::under(Path::new("C:/root"));
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("uninstall.exe"), b"").unwrap();
        let plan = plan(&paths, Some(dir.path()));
        let cmd = plan.run_uninstaller.unwrap();
        assert_eq!(cmd.program, dir.path().join("uninstall.exe"));
        assert_eq!(cmd.args, vec!["/S".to_string()]);
    }

    #[test]
    fn plan_skips_uninstaller_for_portable_or_dev_builds() {
        let paths = AppPaths::under(Path::new("C:/root"));
        let dir = tempfile::tempdir().unwrap(); // no uninstall.exe here
        assert_eq!(plan(&paths, Some(dir.path())).run_uninstaller, None);
    }

    #[test]
    fn after_exit_command_waits_removes_data_then_runs_the_uninstaller() {
        let root = Path::new(r"C:\Users\A B\AppData");
        let paths = AppPaths::under(root);
        let install = tempfile::tempdir().unwrap();
        std::fs::write(install.path().join("uninstall.exe"), b"").unwrap();
        let cmd = plan(&paths, Some(install.path()))
            .after_exit_command()
            .unwrap();
        let wait = cmd.find("ping 127.0.0.1").unwrap();
        let data = cmd
            .find(r#"rmdir /s /q "C:\Users\A B\AppData\data""#)
            .unwrap();
        let config = cmd
            .find(r#"rmdir /s /q "C:\Users\A B\AppData\config""#)
            .unwrap();
        let uninstall = cmd
            .find(&format!(
                "\"{}\" /S",
                install.path().join("uninstall.exe").display()
            ))
            .unwrap();
        assert!(wait < data && data < config && config < uninstall, "{cmd}");
    }

    #[test]
    fn after_exit_command_without_installer_only_removes_data() {
        let paths = AppPaths::under(Path::new(r"C:\root"));
        let cmd = plan(&paths, None).after_exit_command().unwrap();
        assert!(cmd.contains("rmdir") && !cmd.contains("uninstall"), "{cmd}");
    }

    #[test]
    fn refuses_when_a_data_folder_contains_the_installed_program() {
        // Deleting it would remove the program and its uninstaller mid-way.
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(root.path());
        let install = paths.data_dir.join("app");
        let p = plan(&paths, Some(&install));
        assert!(p.after_exit_command().is_err());
    }

    #[test]
    fn refuses_a_directory_outside_the_aural_roots() {
        let paths = AppPaths::under(Path::new(r"C:\root"));
        let mut p = plan(&paths, None);
        p.remove_dirs.push(Path::new(r"C:\Windows").to_path_buf());
        assert!(p.after_exit_command().is_err());
    }

    #[test]
    fn refuses_paths_that_would_break_out_of_the_command() {
        let paths = AppPaths::under(Path::new(r#"C:\x" & del C:\y & ""#));
        assert!(plan(&paths, None).after_exit_command().is_err());
    }

    /// Runs the real command with cmd.exe against temp folders (takes ~2 s).
    #[cfg(windows)]
    #[test]
    fn after_exit_command_really_deletes_the_folders() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(root.path());
        std::fs::create_dir_all(paths.models_dir().join("m")).unwrap();
        std::fs::write(paths.models_dir().join("m").join("w.onnx"), b"x").unwrap();
        std::fs::create_dir_all(&paths.config_dir).unwrap();
        std::fs::write(paths.settings_file(), b"{}").unwrap();
        let cmd = plan(&paths, None).after_exit_command().unwrap();
        use std::os::windows::process::CommandExt;
        let status = std::process::Command::new("cmd.exe")
            .raw_arg("/C")
            .raw_arg(&cmd)
            .status()
            .unwrap();
        assert!(status.success() || !paths.data_dir.exists());
        assert!(!paths.data_dir.exists(), "data folder still there");
        assert!(!paths.config_dir.exists(), "config folder still there");
        assert!(
            root.path().exists(),
            "must not touch anything above the roots"
        );
    }
}
