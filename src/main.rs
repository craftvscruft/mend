use clap::{Parser, Error};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
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
    env: BTreeMap<String, String>,

    #[serde(default)]
    recipes: BTreeMap<String, Recipe>,

    #[serde(default)]
    hooks: BTreeMap<String, Hook>,

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
    run(&Cli::parse());
    std::process::exit(1);
}

fn run(cli: &Cli) {
    if let Ok(merged_mend) = load_mend(cli) {
        if let Ok(text) = toml::to_string_pretty(&merged_mend) {
            println!("{}", text);
        } else {
            exit(1);
        }
    } else {
        exit(1);
    }

}

fn load_mend(cli: &Cli) -> Result<Mend, Error> {
    let parent_dir = Path::new(&cli.file)
        .parent()
        .expect("Unable to get the parent directory");


    let contents = match fs::read_to_string(&cli.file) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Could not read file `{}` {}", &cli.file, e);
            panic!()
        }
    };

    let main_mend: Mend = match toml::from_str(&contents) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Unable to load data from `{}` {}", &cli.file, e);
            panic!()
        }
    };
    let mut merged_mend: Mend = Mend {
        from: None,
        include: Vec::new(),
        env: BTreeMap::new(),
        recipes: BTreeMap::new(),
        hooks: BTreeMap::new(),
        steps: Vec::new(),
    };
    for include_file in &main_mend.include {
        let include_contents = match fs::read_to_string(parent_dir.join(include_file)) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Could not read include file `{}` {}", &include_file, e);
                panic!();
            }
        };
        let include_mend: Mend = match toml::from_str(&include_contents) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("Unable to load data from `{}` {}", &include_file, e);
                panic!();
            }
        };
        if !include_mend.steps.is_empty() {
            eprintln!(
                "We only allow includes 1 level deep, sorry. Please restructure `{}`",
                &include_file
            );
            panic!();
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
    Ok(merged_mend)
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

    use crate::{Cli, load_mend};

    #[test]
    fn it_works() {
        let mut toml_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        toml_path.push("examples/mend.toml");
        let args = Cli {
            file: String::from(toml_path.to_str().unwrap())
        };
        let loaded = load_mend(&args);
        insta::assert_yaml_snapshot!(loaded.expect("Failed loading"));
    }
}