use clap::Parser;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::exit;
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[arg(short='f', long="file", default_value_t=String::from("mend.toml"))]
    file: String,
}
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Mend {
    from: Option<From>,

    #[serde(default)]
    include: Vec<String>,

    #[serde(default)]
    env: HashMap<String, String>,

    #[serde(default)]
    recipes: HashMap<String, Recipe>,

    #[serde(default)]
    hooks: HashMap<String, Hook>,

    #[serde(default)]
    steps: Vec<String>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct From {
    sha: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Recipe {
    run: String,
    commit_template: Option<String>,
    tag: Option<String>,

    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Hook {
    run: Option<String>,
    run_for_tag: Option<HookRunForTag>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct HookRunForTag {
    tag: String,
    run: String,
    else_run: Option<String>,
}

fn main() {
    let cli = Cli::parse();
    let parent_dir = Path::new(&cli.file)
        .parent()
        .expect("Unable to get the parent directory");

    // file.read_to_string(&mut contents).expect("Unable to read the file");

    let contents = match fs::read_to_string(&cli.file) {
        // If successful return the files text as `contents`.
        // `c` is a local variable.
        Ok(c) => c,
        // Handle the `error` case.
        Err(e) => {
            // Write `msg` to `stderr`.
            eprintln!("Could not read file `{}` {}", &cli.file, e);
            // Exit the program with exit code `1`.
            exit(1);
        }
    };

    let main_mend: Mend = match toml::from_str(&contents) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Unable to load data from `{}` {}", &cli.file, e);
            exit(1);
        }
    };
    let mut merged_mend: Mend = Mend {
        from: None,
        include: Vec::new(),
        env: HashMap::new(),
        recipes: HashMap::new(),
        hooks: HashMap::new(),
        steps: Vec::new(),
    };
    for include_file in &main_mend.include {
        let include_contents = match fs::read_to_string(parent_dir.join(&include_file)) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Could not read include file `{}` {}", &include_file, e);
                exit(1);
            }
        };
        let include_mend: Mend = match toml::from_str(&include_contents) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("Unable to load data from `{}` {}", &include_file, e);
                exit(1);
            }
        };
        if !include_mend.steps.is_empty() {
            eprintln!(
                "We only allow includes 1 level deep, sorry. Please restructure `{}`",
                &include_file
            );
            exit(1);
        }
        extend_mend(&mut merged_mend, include_mend);
    }
    extend_mend(&mut merged_mend, main_mend);
    for recipe_entry in merged_mend.recipes.values_mut() {
        match recipe_entry.tag.take() {
            Some(tag) => {
                recipe_entry.tags.push(tag);
                recipe_entry.tag = None
            }
            None => {}
        }
    }

    if let Ok(text) = toml::to_string_pretty(&merged_mend) {
        println!("{}", text);
    }
    std::process::exit(1);
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
