use clap::Parser;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::path::{Path, PathBuf};

use crate::repo::ensure_worktree;
use std::process::exit;

mod config;
mod repo;

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
    hooks: BTreeMap<String, Hook>,

    #[serde(default)]
    steps: Vec<String>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
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
    run_for_tag: Option<HookRunForTag>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct HookRunForTag {
    tag: String,
    run: String,
    else_run: Option<String>,
}

fn main() {
    run(&Cli::parse());
}

fn drive(mend: Mend) {
    let from = mend.from.expect("No from declared in config");
    // repo could be remote but for now assume a local checkout
    let repo_dir_raw = Path::new(&from.repo);
    // Multiple concurrent runs will stomp on each other. Choose unique dir?
    let repo_dir = expand_path(&repo_dir_raw);
    let worktree_dir = ensure_worktree(repo_dir.as_path(), ".mend/worktree2", &from.sha);
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
                drive(merged_mend)
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
    fn it_works() {
        let mut toml_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        toml_path.push("examples/mend.toml");
        let args = Cli {
            file: String::from(toml_path.to_str().unwrap()),
        };
        let loaded = load_mend(&args);
        insta::assert_yaml_snapshot!(loaded.expect("Failed loading"));
    }
}
