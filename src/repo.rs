use anyhow::Context;

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;
use which::which;

pub fn ensure_worktree(repo_dir: &Path, work_dir_relative: &str, sha: &str) -> PathBuf {
    let work_dir_joined = repo_dir.join(work_dir_relative);
    eprintln!(
        "Creating worktree at {} in repo at {}",
        work_dir_joined.to_str().unwrap(),
        repo_dir.to_str().unwrap()
    );

    let git_cmd = which("git").expect("Could resolve git command");
    if work_dir_joined.exists() {
        let _ = Command::new(&git_cmd)
            .current_dir(repo_dir)
            .args(["worktree", "remove", "--force", work_dir_relative])
            .spawn();
    }

    let worktree_result = Command::new(&git_cmd)
        .current_dir(repo_dir)
        .args(["worktree", "add", "--force", work_dir_relative, sha])
        .output();
    if let Err(e) = &worktree_result {
        eprintln!("git_cmd {:?} {:?} {:?}", &git_cmd, &e, &e.source());
    }
    // eprintln!("{:?}", worktree_result);
    worktree_result.expect("Could not open worktree");
    work_dir_joined
}

fn commit_all(repo_dir: &Path, message: &str) {
    let _ = Command::new("git")
        .current_dir(repo_dir)
        .arg("commit")
        .arg("-am")
        .arg(message)
        .spawn()
        .expect("Could not commit");
}

fn reset_hard(repo_dir: &Path) {
    eprintln!("{}", String::from(repo_dir.to_str().unwrap()));
    eprintln!("{}", repo_dir.exists());
    eprintln!("{}", repo_dir.parent().unwrap().exists());

    let _ = Command::new("git")
        .current_dir(repo_dir)
        .args(["reset", "--hard"])
        .spawn()
        .expect("Could not reset");
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
