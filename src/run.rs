use crate::{Mend, Recipe};
use serde::{Deserialize, Serialize};
use std::fmt::{Debug, format};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct RunStatus {
    steps: Vec<StepStatus>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct StepStatus {
    run: String,
    run_resolved: String,
    commit_msg: String,
    sha: Option<String>,
    status: EStatus,
    output: Option<String>
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub enum EStatus {
    Pending, Running, Done, Failed
}

fn resolve_step_instruction(instruction: String, mend: &Mend) -> String {
    let mut resolved_instruction = "".to_owned();
    for (recipe_name, recipe) in &mend.recipes {
        if instruction.contains(recipe_name.as_str()) {
            let recipe_fn = format!("function {} () {{\n{}\n}}\n", recipe_name, recipe.run);
            resolved_instruction.push_str(&recipe_fn)
        }
    }
    resolved_instruction.push_str(&instruction);
    resolved_instruction
}

fn create_run_status_from_mend(mend: Mend) -> RunStatus {
    RunStatus {
        steps: mend.steps.iter().map({|step |
            StepStatus {
                run: step.to_string(),
                run_resolved: resolve_step_instruction(step.to_string(), &mend),
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
    use crate::{Mend, Recipe};
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