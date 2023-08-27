use crate::progress::Notify;
use crate::run::EStatus::{Done, Failed, Running};
use crate::Mend;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::path::Path;
use std::process::{Command, Output};
use which::which;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct RunStatus {
    pub steps: Vec<StepStatus>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct StepStatus {
    pub run: String,
    pub run_resolved: Vec<String>,
    pub commit_msg: String,
    pub sha: Option<String>,
    pub status: EStatus,
    pub output: Option<String>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub enum EStatus {
    Pending,
    Running,
    Done,
    Failed,
}

fn resolve_step_scripts(instruction: String, mend: &Mend) -> Vec<String> {
    let mut resolved_instruction = "".to_owned();
    let mut scripts = vec![];
    let mut recipe_tags: Vec<String> = vec![];
    for (recipe_name, recipe) in &mend.recipes {
        if instruction.contains(recipe_name.as_str()) {
            let recipe_fn = format!("function {}() {{\n{}\n}}\n", recipe_name, recipe.run);
            resolved_instruction.push_str(&recipe_fn);
            for tag in &recipe.tags {
                recipe_tags.push(tag.to_string())
            }
        }
    }
    resolved_instruction.push_str(&instruction);
    resolved_instruction.push('\n');

    add_matching_hooks(&mut scripts, mend, "before_step", &recipe_tags);
    scripts.push(resolved_instruction);
    add_matching_hooks(&mut scripts, mend, "after_step", &recipe_tags);
    scripts
}

pub trait Executor {
    fn run_script(&self, cwd: &Path, script: &str) -> anyhow::Result<Output>;
}

pub struct ShellExecutor {}

impl Executor for ShellExecutor {
    fn run_script(&self, cwd: &Path, script: &str) -> anyhow::Result<Output> {
        run_command_with_output(cwd, "sh".to_string(), vec!["-c", script])
    }
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

pub fn create_run_status_from_mend(mend: &Mend) -> RunStatus {
    RunStatus {
        steps: mend
            .steps
            .iter()
            .map({
                |step| StepStatus {
                    run: step.to_string(),
                    run_resolved: resolve_step_scripts(step.to_string(), mend),
                    commit_msg: step.to_string(),
                    sha: None,
                    status: EStatus::Pending,
                    output: None,
                }
            })
            .collect(),
    }
}

pub fn run_step<E: Executor, N: Notify>(
    step_status: &mut StepStatus,
    cwd: &Path,
    executor: &E,
    notifier: &N,
    step_i: usize,
) {
    step_status.status = Running;
    let mut output_text = "".to_owned();
    let vec = &step_status.run_resolved;
    for script in vec {
        notifier.notify(step_i, step_status, true);
        let output_result = executor.run_script(cwd, script);
        match output_result {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                output_text.push_str(stdout.as_ref());
                output_text.push_str(stderr.as_ref());
                if !output.status.success() {
                    step_status.status = Failed;
                    notifier.notify(step_i, step_status, false);
                    break;
                }
            }
            Err(_e) => {
                step_status.status = Failed;
                notifier.notify(step_i, step_status, false);
            }
        }
    }
    step_status.output = Some(output_text);
    if step_status.status != Failed {
        step_status.status = Done;
        notifier.notify(step_i, step_status, true);
    } else {
        notifier.notify(step_i, step_status, false);
    }
}

pub fn run_command_with_output(
    repo_dir: &Path,
    cmd: String,
    args: Vec<&str>,
) -> anyhow::Result<Output> {
    let cmd_path = which(&cmd).with_context(|| "could not resolve")?;
    Command::new(&cmd_path)
        .current_dir(repo_dir)
        .args(args)
        .output()
        .with_context(|| format!("Could not run command {}, resolved {:?}", cmd, cmd_path))
}

#[cfg(test)]
mod tests {
    use crate::run::create_run_status_from_mend;
    use crate::{Hook, Mend, Recipe};

    #[test]
    fn test_create_run_status_empty() {
        let mend = create_mend_with_steps(vec![]);
        insta::assert_yaml_snapshot!(create_run_status_from_mend(&mend));
    }

    #[test]
    fn test_create_run_status_one_step() {
        let mend = create_mend_with_steps(vec!["cmd arg1 arg2".to_string()]);
        let status = create_run_status_from_mend(&mend);
        assert_eq!(status.steps.len(), 1);
        insta::assert_yaml_snapshot!(status);
    }

    #[test]
    fn test_create_run_status_resolve_recipe() {
        let mut mend = create_mend_with_steps(vec!["cmd arg1 arg2".to_string()]);

        mend.recipes.insert(
            "cmd".to_string(),
            Recipe {
                run: "resolved $1 $2".to_string(),
                commit_template: None,
                tag: None,
                tags: vec![],
            },
        );
        mend.recipes.insert(
            "not_used".to_string(),
            Recipe {
                run: "should not appear!".to_string(),
                commit_template: None,
                tag: None,
                tags: vec![],
            },
        );
        let status = create_run_status_from_mend(&mend);
        assert_eq!(status.steps.len(), 1);
        insta::assert_yaml_snapshot!(status);
    }

    #[test]
    fn test_create_run_status_include_hooks() {
        let mut mend = create_mend_with_steps(vec!["cmd arg1 arg2".to_string()]);

        let before_step_hook = Hook {
            run: Option::from("echo Hello before".to_string()),
            when_tag: None,
            when_not_tag: None,
        };
        let after_step_hook = Hook {
            run: Option::from("echo Hello after".to_string()),
            when_tag: None,
            when_not_tag: None,
        };
        mend.hooks
            .insert("before_step".to_string(), vec![before_step_hook]);
        mend.hooks
            .insert("after_step".to_string(), vec![after_step_hook]);
        let status = create_run_status_from_mend(&mend);
        assert_eq!(status.steps.len(), 1);
        insta::assert_yaml_snapshot!(status);
    }

    #[test]
    fn test_create_run_status_include_hooks_matching_on_tag() {
        let mut mend = create_mend_with_steps(vec!["cmd arg1 arg2".to_string()]);

        let before_hook_tag = Hook {
            run: Option::from("echo Hello before some_tag".to_string()),
            when_tag: Some("some_tag".to_string()),
            when_not_tag: None,
        };
        let before_hook_not_tag = Hook {
            run: Some("echo Hello from before NOT some_tag".to_string()),
            when_tag: None,
            when_not_tag: Some("some_tag".to_string()),
        };
        mend.hooks.insert(
            "before_step".to_string(),
            vec![before_hook_tag, before_hook_not_tag],
        );
        mend.recipes.insert(
            "cmd".to_string(),
            Recipe {
                run: "resolved $1 $2".to_string(),
                commit_template: None,
                tag: None,
                tags: vec!["some_tag".to_string()],
            },
        );
        let status = create_run_status_from_mend(&mend);
        assert_eq!(status.steps.len(), 1);
        insta::assert_yaml_snapshot!(status);
    }

    fn create_mend_with_steps(steps: Vec<String>) -> Mend {
        Mend {
            from: None,
            include: vec![],
            env: Default::default(),
            recipes: Default::default(),
            hooks: Default::default(),
            steps,
        }
    }
}
