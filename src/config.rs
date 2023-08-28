use crate::Mend;
use anyhow::{anyhow, Context};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

pub fn load_mend(file: &Path) -> anyhow::Result<Mend> {
    let file_str = file.to_str().unwrap_or_default();
    let parent_dir = &file.parent().unwrap_or(Path::new(""));

    let contents =
        fs::read_to_string(&file).with_context(|| format!("Could not read file `{}`", file_str))?;

    let main_mend: Mend = toml::from_str(&contents)
        .with_context(|| format!("Unable to load data from `{}`", file_str))?;

    let mut merged_mend: Mend = Mend {
        from: None,
        include: Vec::new(),
        env: BTreeMap::new(),
        recipes: BTreeMap::new(),
        hooks: BTreeMap::new(),
        steps: Vec::new(),
    };
    for include_file in &main_mend.include {
        let include_contents =
            fs::read_to_string(parent_dir.join(include_file)).with_context(|| {
                format!(
                    "Could not read include file `{}` included from `{}`",
                    &include_file, file_str
                )
            })?;
        let include_mend: Mend = toml::from_str(&include_contents)
            .with_context(|| format!("Unable to load data from `{}`", &include_file))?;
        if !include_mend.steps.is_empty() {
            return Err(anyhow!(
                "We only allow includes 1 level deep, sorry. Please restructure `{}`",
                &include_file
            ));
        }
        crate::extend_mend(&mut merged_mend, include_mend);
    }
    crate::extend_mend(&mut merged_mend, main_mend);
    for recipe_entry in merged_mend.recipes.values_mut() {
        // This allows users to specify either single "tag" or multiple "tags".
        // Probably should be handled on the deserialization side
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

#[cfg(test)]
mod tests {
    use crate::config::load_mend;
    use std::path::PathBuf;

    fn path_from_manifest(rel_path: &str) -> PathBuf {
        let mut toml_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        toml_path.push(rel_path);
        toml_path
    }

    #[test]
    fn load_mend_from_toml() {
        let toml_path = path_from_manifest("examples/mend.toml");
        let loaded = load_mend(toml_path.as_path());
        insta::assert_yaml_snapshot!(loaded.expect("Failed loading"));
    }
}
