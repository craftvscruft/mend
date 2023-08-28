use crate::run::run_command_with_output;
use anyhow::{bail, Context};
use std::path::{Path, PathBuf};
use std::process::Command;

pub trait Repo {
    fn commit_all(&mut self, message: &str) -> anyhow::Result<()>;
    fn reset_hard(&mut self) -> anyhow::Result<()>;
    fn current_short_sha(&self) -> anyhow::Result<String>;
    fn dir(&self) -> &Path;
}

pub fn ensure_worktree(
    repo_dir: &Path,
    work_dir_relative: &str,
    sha: &str,
) -> anyhow::Result<PathBuf> {
    let work_dir_joined = repo_dir.join(work_dir_relative);
    // eprintln!(
    //     "Creating worktree at {} in repo at {}",
    //     work_dir_joined.to_str().unwrap(),
    //     repo_dir.to_str().unwrap()
    // );

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

pub struct GitRepo {
    pub repo_dir: PathBuf,
}

impl Repo for GitRepo {
    fn dir(&self) -> &Path {
        &self.repo_dir
    }
    fn commit_all(&mut self, message: &str) -> anyhow::Result<()> {
        let output = run_command_with_output(
            &self.repo_dir,
            "git".to_string(),
            vec!["commit", "-am", message],
        )?;
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

    fn reset_hard(&mut self) -> anyhow::Result<()> {
        let output =
            run_command_with_output(&self.repo_dir, "git".to_string(), vec!["reset", "--hard"])?;
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

    fn current_short_sha(&self) -> anyhow::Result<String> {
        let output = Command::new("git")
            .current_dir(&self.repo_dir)
            .args(["rev-parse", "--short", "HEAD"])
            .output()
            .expect("Could not get sha");
        Ok(String::from_utf8(output.stdout)
            .with_context(|| "Could not get sha")?
            .trim()
            .parse()?)
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::process::Command;
    use tempfile::tempdir_in;

    use crate::repo::{ensure_worktree, GitRepo, Repo};

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
        let base_repo_dir = temp_dir.path();

        let _ = Command::new("git")
            .current_dir(base_repo_dir)
            .arg("init")
            .output()
            .expect("Could not init");
        let _ = File::create(base_repo_dir.join("myfile")).unwrap();

        let _ = Command::new("git")
            .current_dir(base_repo_dir)
            .args(["add", "myfile"])
            .output()
            .expect("Could not git add  myfile");
        let mut base_repo = GitRepo {
            repo_dir: base_repo_dir.to_path_buf(),
        };
        base_repo.commit_all("Initial").expect("Could not commit");

        let short_sha = base_repo.current_short_sha().unwrap();
        let worktree_dir =
            ensure_worktree(base_repo_dir, worktree_rel, short_sha.as_str()).unwrap();
        let mut worktree_repo = GitRepo {
            repo_dir: worktree_dir,
        };
        worktree_repo.reset_hard().expect("Could not git reset");

        assert_eq!(short_sha, worktree_repo.current_short_sha().unwrap());
        // Can call ensure_worktree twice on the same directory
        ensure_worktree(base_repo_dir, worktree_rel, short_sha.as_str())
            .expect("could not create worktree");
        assert_eq!(short_sha, worktree_repo.current_short_sha().unwrap());
        // Hold onto references
        let _ = temp_subdir.close();
        let _ = temp_dir.close();
    }
}
