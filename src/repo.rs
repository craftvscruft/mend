use anyhow::{bail, Context};


use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use which::which;

pub fn ensure_worktree(
    repo_dir: &Path,
    work_dir_relative: &str,
    sha: &str,
) -> anyhow::Result<PathBuf> {
    let work_dir_joined = repo_dir.join(work_dir_relative);
    eprintln!(
        "Creating worktree at {} in repo at {}",
        work_dir_joined.to_str().unwrap(),
        repo_dir.to_str().unwrap()
    );

    if work_dir_joined.exists() {
        run_command_with_output(
            repo_dir,
            "git".to_string(),
            vec!["worktree", "remove", "--force", work_dir_relative],
        )?;
    }

    let output = run_command_with_output(
        repo_dir,
        "git".to_string(),
        vec!["worktree", "add", "--force", work_dir_relative, sha],
    )?;
    if !output.status.success() {
        bail!(
            "Failed to create worktree, output:\n{}{}",
            String::from_utf8_lossy(&output.stdout).as_ref(),
            String::from_utf8_lossy(&output.stderr).as_ref()
        );
    }
    Ok(work_dir_joined)
}

fn run_command_with_output(
    repo_dir: &Path,
    cmd: String,
    args: Vec<&str>,
) -> anyhow::Result<Output> {
    let cmd_path = which(cmd).with_context(|| "could not resolve")?;
    let child = Command::new(cmd_path)
        .current_dir(repo_dir)
        .args(args)
        .spawn()
        .with_context(|| "")?;
    child.wait_with_output().with_context(|| "")
}

fn commit_all(repo_dir: &Path, message: &str) -> anyhow::Result<()> {
    let output =
        run_command_with_output(repo_dir, "git".to_string(), vec!["commit", "-am", message])?;
    if !output.status.success() {
        bail!(
            "Failed to commit, output:\n{}{}",
            String::from_utf8_lossy(&output.stdout).as_ref(),
            String::from_utf8_lossy(&output.stderr).as_ref()
        );
    } else {
        Ok(())
    }
}

fn reset_hard(repo_dir: &Path) -> anyhow::Result<()> {
    let output = run_command_with_output(repo_dir, "git".to_string(), vec!["reset", "--hard"])?;
    if !output.status.success() {
        bail!(
            "Failed to commit, output:\n{}{}",
            String::from_utf8_lossy(&output.stdout).as_ref(),
            String::from_utf8_lossy(&output.stderr).as_ref()
        );
    } else {
        Ok(())
    }
}

fn current_short_sha(repo_dir: &Path) -> anyhow::Result<String> {
    let output = Command::new("git")
        .current_dir(repo_dir)
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .expect("Could not get sha");
    Ok(String::from_utf8(output.stdout)
        .with_context(|| "Could not get sha")?
        .trim()
        .parse()?)
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::process::Command;
    use tempfile::tempdir_in;

    use crate::repo::{commit_all, current_short_sha, ensure_worktree, reset_hard};

    #[test]
    fn git_commands() {
        let temp_dir = tempfile::tempdir().unwrap();
        let temp_subdir = tempdir_in(&temp_dir).unwrap();
        let worktree_rel = temp_subdir
            .path()
            .strip_prefix(temp_dir.path())
            .unwrap()
            .to_str()
            .unwrap();
        let repo_dir = temp_dir.path();
        let worktree_dir = repo_dir.join(worktree_rel);

        let _ = Command::new("git")
            .current_dir(repo_dir)
            .arg("init")
            .spawn()
            .expect("Could not init");
        let _ = File::create(repo_dir.join("myfile")).unwrap();

        let _ = Command::new("git")
            .current_dir(repo_dir)
            .args(["add", "myfile"])
            .spawn()
            .expect("Could not git add  myfile");

        commit_all(repo_dir, "Initial");

        let short_sha = current_short_sha(repo_dir).unwrap();
        ensure_worktree(repo_dir, worktree_rel, short_sha.as_str());
        reset_hard(&worktree_dir);

        assert_eq!(short_sha, current_short_sha(&worktree_dir).unwrap());
        // Can call ensure_worktree twice on the same directory
        ensure_worktree(repo_dir, worktree_rel, short_sha.as_str());
        assert_eq!(short_sha, current_short_sha(&worktree_dir).unwrap());
        // Hold onto references
        let _ = temp_subdir.close();
        let _ = temp_dir.close();
    }
}
