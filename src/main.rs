use anyhow::bail;
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::fmt::Debug;
use std::path::{Path, PathBuf};

use crate::progress::{create_console_notifier, Notify};
use crate::repo::{ensure_worktree, GitRepo, Repo};
use crate::run::EStatus::Failed;
use crate::run::{create_run_status_from_mend, EStatus, Executor, run_step, ShellExecutor, StepRequest, StepResponse};

mod config;
mod progress;
mod repo;
mod run;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[arg(short = 'f', long = "file")]
    pub file: Option<String>,

    #[arg(long = "dry-run")]
    pub dry_run: bool,
}
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Mend {
    from: Option<From>,

    #[serde(default)]
    include: Vec<String>,

    #[serde(default)]
    env: BTreeMap<String, String>,

    #[serde(default)]
    recipes: BTreeMap<String, Recipe>,

    #[serde(default)]
    hooks: BTreeMap<String, Vec<Hook>>,

    #[serde(default)]
    steps: Vec<String>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
pub struct From {
    sha: String,
    repo: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Recipe {
    run: String,
    commit_template: Option<String>,
    tag: Option<String>,

    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Hook {
    run: Option<String>,
    when_tag: Option<String>,
    when_not_tag: Option<String>,
}

fn main() {
    match run(&Cli::parse()) {
        Ok(_) => {
            std::process::exit(0);
        }
        Err(err) => {
            eprintln!("{:#}", err);
            std::process::exit(1);
        }
    }
}

fn drive(mend: &Mend) {
    let from = mend
        .from
        .as_ref()
        .expect("No from declared in config")
        .clone();
    let step_requests = create_run_status_from_mend(mend);
    let mut notifier = create_console_notifier(&step_requests);
    // repo could be remote but for now assume a local checkout
    let repo_dir_raw = Path::new(&from.repo);
    // Multiple concurrent runs will stomp on each other. Choose unique dir?
    let base_repo_dir = expand_path(repo_dir_raw);

    if let Ok(worktree_dir) = ensure_worktree(base_repo_dir.as_path(), ".mend/worktree2", &from.sha)
    {
        if !worktree_dir.exists() {
            eprintln!(
                "Worktree dir {} doesn't exist",
                worktree_dir.to_string_lossy()
            );
        }
        let mut worktree_repo = GitRepo {
            repo_dir: worktree_dir,
        };
        for (key, value) in &mend.env {
            let expanded = shellexpand::env(value).unwrap();
            env::set_var(key, expanded.as_ref());
        }

        let mut executor = ShellExecutor {};
        run_all_steps(step_requests, &mut notifier, &mut worktree_repo, &mut executor);
        notifier.notify_done()
    }
}

fn run_all_steps<R: Repo, E: Executor, N: Notify>(step_requests: Vec<StepRequest>, notifier: &mut N, worktree_repo: &mut R, executor: &mut E) {
    let mut step_i: usize = 0;
    for step_request in step_requests {
        let mut step_response = StepResponse { sha: None, status: EStatus::Pending, output: None };
        run_step(
            worktree_repo,
            executor,
            notifier,
            step_i,
            &step_request,
            &mut step_response,
        );
        step_i += 1;
        if step_response.status == Failed {
            println!("Failed on {:?}", step_request);
            println!("Response {:?}", step_response);
            break;
        }
    }
}

fn expand_path(repo_dir_raw: &Path) -> PathBuf {
    let cow = shellexpand::path::full(&repo_dir_raw).expect("Cannot resolve path");
    cow.to_path_buf()
}

fn run(cli: &Cli) -> anyhow::Result<()> {
    let config_path = match &cli.file {
        Some(file) => {
            let path = Path::new(file.as_str());
            if path.exists() {
                path
            } else {
                bail!("Specified file {} doesn't exist", file)
            }
        }
        None => {
            let toml_path = Path::new("mend.toml");
            if toml_path.exists() {
                toml_path
            } else {
                bail!(
                    "No mend.toml found, please specify one with -f or create one with `mend init`"
                )
            }
        }
    };
    let merged_mend = config::load_mend(config_path)?;
    if cli.dry_run {
        eprintln!("Dry run, skipping")
    } else {
        drive(&merged_mend)
    }
    Ok(())
}

fn extend_mend(merged_mend: &mut Mend, include_mend: Mend) {
    merged_mend.env.extend(include_mend.env);
    merged_mend.from = include_mend.from;
    merged_mend.recipes.extend(include_mend.recipes);
    merged_mend.hooks.extend(include_mend.hooks);
    for ele in include_mend.steps {
        merged_mend.steps.push(ele)
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use std::env;
    use std::path::PathBuf;

    use crate::config::load_mend;
    use crate::{run, Cli};

    fn path_from_manifest(rel_path: &str) -> PathBuf {
        let mut toml_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        toml_path.push(rel_path);
        toml_path
    }

    fn strip_manifest_path_from_text(text: String) -> String {
        // So that snapshots won't differ in tests run on different machines
        text.replace(env!("CARGO_MANIFEST_DIR"), "<MANIFEST_DIR>")
    }

    #[test]
    fn load_mend_from_toml() {
        let toml_path = path_from_manifest("examples/mend.toml");
        let loaded = load_mend(toml_path.as_path());
        insta::assert_yaml_snapshot!(loaded.expect("Failed loading"));
    }

    #[test]
    fn cli_parse_dry_run() {
        let cli = Cli::parse_from(vec!["mend", "--dry-run"]);
        assert!(cli.dry_run);
    }

    #[test]
    fn cli_fails_loading_default_file() {
        // Change out of current dir in case we have a mend.toml there.
        assert!(env::set_current_dir(&path_from_manifest("tests/data")).is_ok());
        let result = run(&Cli::parse_from(vec!["mend", "--dry-run"]));
        assert!(result.is_err());
        insta::assert_snapshot!(format!("{:#}", result.err().unwrap()));
    }

    #[test]
    fn cli_fails_loading_specified_file() {
        let result = run(&Cli::parse_from(vec![
            "mend",
            "--dry-run",
            "-f",
            path_from_manifest("tests/data/not-there.toml")
                .to_str()
                .unwrap(),
        ]));
        assert!(result.is_err());
        insta::assert_snapshot!(strip_manifest_path_from_text(format!("{:#}", result.err().unwrap())));
    }

    #[test]
    fn cli_fails_loading_missing_include() {
        let result = run(&Cli::parse_from(vec![
            "mend",
            "--dry-run",
            "-f",
            path_from_manifest("tests/data/missing-include.toml")
                .to_str()
                .unwrap(),
        ]));
        assert!(result.is_err());
        insta::assert_snapshot!(strip_manifest_path_from_text(format!("{:#}", result.err().unwrap())));
    }
}
