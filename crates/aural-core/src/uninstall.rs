//! "Delete Aural": the confirmation rule and the list of what gets removed. The app
//! executes the plan; keeping it pure makes the guard and the scope testable.

use crate::paths::AppPaths;
use anyhow::{bail, Context, Result};
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
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct RemovalReport {
    pub removed: Vec<PathBuf>,
}

/// Delete the planned directories. Refuses anything that is not one of Aural's roots,
/// so a corrupted plan can never delete arbitrary folders.
pub fn remove_dirs(plan: &DeletionPlan) -> Result<RemovalReport> {
    for dir in &plan.remove_dirs {
        if !plan.allowed_roots.iter().any(|r| r == dir) {
            bail!("refusing to delete {}: not an Aural folder", dir.display());
        }
    }
    let mut report = RemovalReport::default();
    for dir in &plan.remove_dirs {
        match std::fs::remove_dir_all(dir) {
            Ok(()) => report.removed.push(dir.clone()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("deleting {}", dir.display())),
        }
    }
    Ok(report)
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
    fn execute_removes_planned_dirs_and_tolerates_missing_ones() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(root.path());
        std::fs::create_dir_all(paths.models_dir().join("m1")).unwrap();
        std::fs::write(paths.models_dir().join("m1/weights.onnx"), b"x").unwrap();
        // config dir intentionally missing
        let p = plan(&paths, None);
        let report = remove_dirs(&p).unwrap();
        assert!(!paths.data_dir.exists());
        assert_eq!(report.removed, vec![paths.data_dir.clone()]);
    }

    #[test]
    fn refuses_to_remove_a_directory_outside_the_aural_roots() {
        let paths = AppPaths::under(Path::new("C:/root"));
        let mut p = plan(&paths, None);
        p.remove_dirs.push(Path::new("C:/Windows").to_path_buf());
        assert!(remove_dirs(&p).is_err());
    }
}
