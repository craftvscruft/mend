use clap::Parser;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::fmt::Debug;
use std::path::{Path, PathBuf};

use crate::repo::ensure_worktree;
use crate::run::EStatus::Failed;
use crate::run::{create_run_status_from_mend, run_step, StepStatus};
use std::process::exit;

mod config;
mod repo;
mod run;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[arg(short='f', long="file", default_value_t=String::from("mend.toml"))]
    file: String,
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
    run(&Cli::parse());
}

fn drive(mend: &Mend) {
    let from = mend
        .from
        .as_ref()
        .expect("No from declared in config")
        .clone();
    // repo could be remote but for now assume a local checkout
    let repo_dir_raw = Path::new(&from.repo);
    // Multiple concurrent runs will stomp on each other. Choose unique dir?
    let repo_dir = expand_path(repo_dir_raw);
    if let Ok(worktree_dir) = ensure_worktree(repo_dir.as_path(), ".mend/worktree2", &from.sha) {
        if !worktree_dir.exists() {
            eprintln!(
                "Worktree dir {} doesn't exist",
                worktree_dir.to_string_lossy()
            );
        }
        let mut run_status = create_run_status_from_mend(&mend);

        for (key, value) in &mend.env {
            let expanded = shellexpand::env(value).unwrap();
            env::set_var(key, expanded.as_ref());
        }
        for mut step_status in run_status.steps {
            println!("Starting: {}", &step_status.run);
            run_step(&mut step_status, &worktree_dir);

            if step_status.status == Failed {
                println!("{:?}", step_status);
                break;
            }
            println!("...Done")
        }
    }
}

fn expand_path(repo_dir_raw: &Path) -> PathBuf {
    let cow = shellexpand::path::full(&repo_dir_raw).expect("Cannot resolve path");
    cow.to_path_buf()
}

fn run(cli: &Cli) {
    match config::load_mend(cli) {
        Ok(merged_mend) => match toml::to_string_pretty(&merged_mend) {
            Ok(text) => {
                println!("{}", text);
                drive(&merged_mend)
            }
            Err(e) => {
                eprintln!("{e}");
                exit(1);
            }
        },
        Err(e) => {
            eprintln!("{e}");
            exit(1);
        }
    }
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
    use std::path::PathBuf;

    use crate::config::load_mend;
    use crate::Cli;

    #[test]
    fn load_mend_from_toml() {
        let mut toml_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        toml_path.push("examples/mend.toml");
        let args = Cli {
            file: String::from(toml_path.to_str().unwrap()),
        };
        let loaded = load_mend(&args);
        insta::assert_yaml_snapshot!(loaded.expect("Failed loading"));
    }
}
