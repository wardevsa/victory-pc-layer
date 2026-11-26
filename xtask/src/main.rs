//! Repository maintenance. Run with `cargo xtask <command>`.

mod policy;

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "xtask")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Enforce the asset policy: the repository holds only small text files
    /// we wrote. Checks tracked and untracked-but-not-ignored files.
    Policy {
        /// Check only what is staged for commit (used by the pre-commit hook).
        #[arg(long)]
        staged: bool,
    },
    /// Point git at .githooks/ so every commit runs the policy check.
    InstallHooks,
}

fn main() -> Result<()> {
    let root = workspace_root();
    match Cli::parse().command {
        Cmd::Policy { staged } => policy::run(&root, staged),
        Cmd::InstallHooks => {
            let status = Command::new("git")
                .args(["config", "core.hooksPath", ".githooks"])
                .current_dir(&root)
                .status()
                .context("running git")?;
            anyhow::ensure!(
                status.success(),
                "git config failed (is this a git repository?)"
            );
            println!("hooks installed: commits now run `cargo xtask policy --staged`");
            Ok(())
        }
    }
}

/// Find the repository root at run time so the hook works even when the repo
/// has moved to a different drive letter or is used from a git worktree.
///
/// Strategy (in order):
/// 1. Ask `git rev-parse --show-toplevel` — fast and always right inside a
///    worktree.
/// 2. Walk up from the current directory looking for `.victory-root`.
/// 3. Fall back to the compile-time `CARGO_MANIFEST_DIR/..` so that
///    `cargo xtask` from inside the workspace still works in edge cases.
fn workspace_root() -> PathBuf {
    // 1. git
    if let Ok(out) = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        && out.status.success()
    {
        let s = String::from_utf8_lossy(&out.stdout);
        let p = PathBuf::from(s.trim());
        if p.join(".victory-root").exists() {
            return p;
        }
    }

    // 2. Walk up from cwd
    if let Ok(cwd) = std::env::current_dir() {
        let mut dir = cwd.as_path();
        loop {
            if dir.join(".victory-root").exists() {
                return dir.to_path_buf();
            }
            match dir.parent() {
                Some(p) => dir = p,
                None => break,
            }
        }
    }

    // 3. Compile-time fallback
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives in the workspace")
        .into()
}

#[cfg(test)]
mod tests {
    use super::workspace_root;

    /// workspace_root() must return a path that actually contains .victory-root.
    /// This fails when only the compile-time CARGO_MANIFEST_DIR is used and the
    /// repo has moved to a different location.
    #[test]
    fn workspace_root_contains_marker() {
        let root = workspace_root();
        assert!(
            root.join(".victory-root").exists(),
            "workspace_root() returned {:?} which does not contain .victory-root",
            root
        );
    }
}
