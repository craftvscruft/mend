use crate::Mend;
use serde::{Deserialize, Serialize};
use std::fmt::{Debug};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct RunStatus {
    steps: Vec<StepStatus>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct StepStatus {
    run: String,
    run_resolved: Vec<String>,
    commit_msg: String,
    sha: Option<String>,
    status: EStatus,
    output: Option<String>
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub enum EStatus {
    Pending, Running, Done, Failed
}

fn resolve_step_scripts(instruction: String, mend: &Mend) -> Vec<String> {
    let mut resolved_instruction = "".to_owned();
    let mut scripts = vec![];
    let mut recipe_tags : Vec<String> = vec![];
    for (recipe_name, recipe) in &mend.recipes {
        if instruction.contains(recipe_name.as_str()) {
            let recipe_fn = format!("function {} () {{\n{}\n}}\n", recipe_name, recipe.run);
            resolved_instruction.push_str(&recipe_fn);
            for tag in &recipe.tags {
                recipe_tags.push(tag.to_string())
            }
        }
    }
    resolved_instruction.push_str(&instruction);
    resolved_instruction.push_str("\n");

    add_matching_hooks( &mut scripts, mend, "before_step", &recipe_tags);
    scripts.push(resolved_instruction);
    add_matching_hooks( &mut scripts, mend, "after_step", &recipe_tags);
    scripts
}



fn add_matching_hooks(scripts: &mut Vec<String>, mend: &Mend, key: &str, tags: &Vec<String>) {
    if let Some(hooks) = mend.hooks.get(key) {
        for hook in hooks {
            if let Some(hook_run) = &hook.run {
                if let Some(when_tag) = &hook.when_tag {
                    if tags.contains(when_tag) {
                        scripts.push(hook_run.to_string());
                    }
                } else if let Some(when_not_tag) = &hook.when_not_tag {
                    if !tags.contains(when_not_tag) {
                        scripts.push(hook_run.to_string());
                    }
                } else {
                    scripts.push(hook_run.to_string());
                }
            }
        }
    }
}

fn create_run_status_from_mend(mend: Mend) -> RunStatus {
    RunStatus {
        steps: mend.steps.iter().map({|step |
            StepStatus {
                run: step.to_string(),
                run_resolved: resolve_step_scripts(step.to_string(), &mend),
                commit_msg: step.to_string(),
                sha: None,
                status: EStatus::Pending,
                output: None,
            }
        }).collect()
    }
}

#[cfg(test)]
mod tests {
    use std::os::macos::raw::stat;
    use crate::{Hook, Mend, Recipe};
    use crate::run::create_run_status_from_mend;

    #[test]
    fn test_create_run_status_empty() {
        let mend = create_mend_with_steps(vec![]);
        insta::assert_yaml_snapshot!(create_run_status_from_mend(mend));
    }

    #[test]
    fn test_create_run_status_one_step() {
        let mend = create_mend_with_steps(vec![
            "cmd arg1 arg2".to_string()
        ]);
        let status = create_run_status_from_mend(mend);
        assert_eq!(status.steps.len(), 1);
        insta::assert_yaml_snapshot!(status);
    }

    #[test]
    fn test_create_run_status_resolve_recipe() {
        let mut mend = create_mend_with_steps(vec![
            "cmd arg1 arg2".to_string()
        ]);

        mend.recipes.insert("cmd".to_string(), Recipe {
            run: "resolved $1 $2".to_string(),
            commit_template: None,
            tag: None,
            tags: vec![],
        });
        mend.recipes.insert("not_used".to_string(), Recipe {
            run: "should not appear!".to_string(),
            commit_template: None,
            tag: None,
            tags: vec![],
        });
        let status = create_run_status_from_mend(mend);
        assert_eq!(status.steps.len(), 1);
        insta::assert_yaml_snapshot!(status);
    }

    #[test]
    fn test_create_run_status_include_hooks() {
        let mut mend = create_mend_with_steps(vec![
            "cmd arg1 arg2".to_string()
        ]);

        let before_step_hook = Hook {
            run: Option::from("echo Hello before".to_string()),
            when_tag: None,
            when_not_tag: None
        };
        let after_step_hook = Hook {
            run: Option::from("echo Hello after".to_string()),
            when_tag: None,
            when_not_tag: None
        };
        mend.hooks.insert("before_step".to_string(), vec![before_step_hook]);
        mend.hooks.insert("after_step".to_string(), vec![after_step_hook]);
        let status = create_run_status_from_mend(mend);
        assert_eq!(status.steps.len(), 1);
        insta::assert_yaml_snapshot!(status);
    }

    #[test]
    fn test_create_run_status_include_hooks_matching_on_tag() {
        let mut mend = create_mend_with_steps(vec![
            "cmd arg1 arg2".to_string()
        ]);

        let before_hook_tag = Hook {
            run: Option::from("echo Hello before some_tag".to_string()),
            when_tag: Some("some_tag".to_string()),
            when_not_tag: None
        };
        let before_hook_not_tag = Hook {
            run: Some("echo Hello from before NOT some_tag".to_string()),
            when_tag: None,
            when_not_tag: Some("some_tag".to_string())
        };
        mend.hooks.insert("before_step".to_string(), vec![before_hook_tag, before_hook_not_tag]);
        mend.recipes.insert("cmd".to_string(), Recipe {
            run: "resolved $1 $2".to_string(),
            commit_template: None,
            tag: None,
            tags: vec!["some_tag".to_string()],
        });
        let status = create_run_status_from_mend(mend);
        assert_eq!(status.steps.len(), 1);
        insta::assert_yaml_snapshot!(status);
    }

    fn create_mend_with_steps(steps: Vec<String>) -> Mend {
        let mend = Mend {
            from: None,
            include: vec![],
            env: Default::default(),
            recipes: Default::default(),
            hooks: Default::default(),
            steps: steps,
        };
        mend
    }
}