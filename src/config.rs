use std::path::Path;
use std::fs;
use std::collections::BTreeMap;
use anyhow::{anyhow, Context};
use crate::{Cli, Mend};

pub fn load_mend(cli: &Cli) ->anyhow::Result<Mend> {
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

    let main_mend: Mend = toml::from_str(&contents)
    .with_context(|| format!("Unable to load data from `{}`", &cli.file))?;

    let mut merged_mend: Mend = Mend {
        from: None,
        include: Vec::new(),
        env: BTreeMap::new(),
        recipes: BTreeMap::new(),
        hooks: BTreeMap::new(),
        steps: Vec::new(),
    };
    for include_file in &main_mend.include {
        let include_contents = fs::read_to_string(parent_dir.join(include_file))
            .with_context(|| format!("Could not read include file `{}`", &include_file))?;
        let include_mend: Mend = toml::from_str(&include_contents)
            .with_context(|| format!("Unable to load data from `{}`", &include_file))?;
        if !include_mend.steps.is_empty() {
            return Err(anyhow!("We only allow includes 1 level deep, sorry. Please restructure `{}`", &include_file))
        }
        crate::extend_mend(&mut merged_mend, include_mend);
    }
    crate::extend_mend(&mut merged_mend, main_mend);
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
