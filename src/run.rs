use crate::progress::Notify;
use crate::repo::Repo;
use crate::run::EStatus::{Done, Failed, Running};
use crate::Mend;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::path::Path;
use std::process::{Command, Output};
use which::which;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct StepRequest {
    pub run: String,
    pub run_resolved: Vec<String>,
    pub commit_msg: String
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct StepResponse {
    pub sha: Option<String>,
    pub status: EStatus,
    pub output: Option<String>
}

impl StepResponse {
    pub fn push_output_str(&mut self, text: &str) {
        match &self.output {
            None => self.output = Some(text.to_string()),
            Some(prev_text) => self.output = Some(format!("{}\n{}", prev_text, text)),
        }
    }
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
    fn run_script(&mut self, cwd: &Path, script: &str) -> anyhow::Result<Output>;
}

pub struct ShellExecutor {}

impl Executor for ShellExecutor {
    fn run_script(&mut self, cwd: &Path, script: &str) -> anyhow::Result<Output> {
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

pub fn create_run_status_from_mend(mend: &Mend) -> Vec<StepRequest> {
    mend
            .steps
            .iter()
            .map({
                |step_text| StepRequest {
                    run: step_text.to_string(),
                    run_resolved: resolve_step_scripts(step_text.to_string(), mend),
                    commit_msg: step_text.to_string()
                }
            }).collect()
}

pub fn run_step<R: Repo, E: Executor, N: Notify>(
    repo: &mut R,
    executor: &mut E,
    notifier: &mut N,
    step_i: usize,
    step_request: &StepRequest,
    step_response: &mut StepResponse,
) {
    step_response.status = Running;
    for script in &step_request.run_resolved {
        notifier.notify(
            step_i,
            &step_request.run,
            &step_response.status,
            &step_response.sha,
            true,
        );
        step_response.push_output_str(format!("Running\n{}\n", script).as_str());
        let output_result = executor.run_script(repo.dir(), script);
        match output_result {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                step_response.push_output_str(stdout.as_ref());
                step_response.push_output_str(stderr.as_ref());
                if !output.status.success() {
                    step_response.status = Failed;
                    notifier.notify(
                        step_i,
                        &step_request.run,
                        &step_response.status,
                        &step_response.sha,
                        false,
                    );
                    break;
                }
            }
            Err(e) => {
                step_response.push_output_str(format!("Failed to run\n{:?}", e).as_str());
                step_response.status = Failed;
                notifier.notify(
                    step_i,
                    &step_request.run,
                    &step_response.status,
                    &step_response.sha,
                    false,
                );
            }
        }
    }

    if step_response.status != Failed {
        step_response.status = Done;
        step_response.push_output_str("Committing...");
        match repo.commit_all(step_request.commit_msg.as_str()) {
            Ok(_) => {
                if let Ok(sha) = repo.current_short_sha() {
                    step_response.sha = Some(sha)
                }
            }
            Err(err) => {
                step_response.push_output_str(format!("{:?}", err).as_str());
                step_response.status = Failed
            }
        }
        notifier.notify(
            step_i,
            &step_request.run,
            &step_response.status,
            &step_response.sha,
            true,
        );
    } else {
        let _ = repo.reset_hard();
        notifier.notify(
            step_i,
            &step_request.run,
            &step_response.status,
            &step_response.sha,
            false,
        );
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
    use crate::progress::Notify;
    use crate::repo::Repo;
    use crate::run::{create_run_status_from_mend, run_command_with_output, run_step, EStatus, Executor, StepRequest, StepResponse};
    use crate::{Hook, Mend, Recipe};
    use std::borrow::Borrow;
    use std::cell::RefCell;
    use std::env;
    use std::path::Path;
    use std::process::Output;
    use std::rc::Rc;

    #[test]
    fn test_create_run_status_empty() {
        let mend = create_mend_with_steps(vec![]);
        insta::assert_yaml_snapshot!(create_run_status_from_mend(&mend));
    }

    #[test]
    fn test_create_run_status_one_step() {
        let mend = create_mend_with_steps(vec!["cmd arg1 arg2".to_string()]);
        let step_requests = create_run_status_from_mend(&mend);
        assert_eq!(step_requests.len(), 1);
        insta::assert_yaml_snapshot!(step_requests);
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
        let step_requests = create_run_status_from_mend(&mend);
        assert_eq!(step_requests.len(), 1);
        insta::assert_yaml_snapshot!(step_requests);
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
        let step_requests = create_run_status_from_mend(&mend);
        assert_eq!(step_requests.len(), 1);
        insta::assert_yaml_snapshot!(step_requests);
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
        let step_requests = create_run_status_from_mend(&mend);
        assert_eq!(step_requests.len(), 1);
        insta::assert_yaml_snapshot!(step_requests);
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

    struct FakeRepo {
        logger: Rc<RefCell<TestLogger>>,
    }
    impl Repo for FakeRepo {
        fn commit_all(&mut self, message: &str) -> anyhow::Result<()> {
            let logger_ref_cell: &RefCell<TestLogger> = self.logger.borrow();
            logger_ref_cell
                .borrow_mut()
                .log(format!("Repo commit all with msg '{}'", message));
            Ok(())
        }

        fn reset_hard(&mut self) -> anyhow::Result<()> {
            let logger_ref_cell: &RefCell<TestLogger> = self.logger.borrow();
            logger_ref_cell
                .borrow_mut()
                .log("Repo reset hard".to_string());
            Ok(())
        }

        fn current_short_sha(&self) -> anyhow::Result<String> {
            Ok("..SHA..".to_string())
        }

        fn dir(&self) -> &Path {
            Path::new("some_path")
        }
    }
    struct FakeExecutor {
        logger: Rc<RefCell<TestLogger>>,
        succeed: bool,
    }

    impl Executor for FakeExecutor {
        fn run_script(&mut self, _cwd: &Path, script: &str) -> anyhow::Result<Output> {
            let cmd = if self.succeed {
                "echo".to_string()
            } else {
                "false".to_string()
            };
            let logger_ref_cell: &RefCell<TestLogger> = self.logger.borrow();
            logger_ref_cell
                .borrow_mut()
                .log(format!("Executor run script:\n{}\n", script));
            return run_command_with_output(env::current_dir().unwrap().as_path(), cmd, vec![]);
        }
    }
    struct FakeNotifier {
        logger: Rc<RefCell<TestLogger>>,
    }
    impl Notify for FakeNotifier {
        fn notify(
            &mut self,
            i: usize,
            _run: &str,
            status: &EStatus,
            _sha: &Option<String>,
            inc: bool,
        ) {
            let logger_ref_cell: &RefCell<TestLogger> = self.logger.borrow();
            logger_ref_cell
                .borrow_mut()
                .log(format!("Notify step {} status {:?} inc {}", i, status, inc))
        }

        fn notify_done(&self) {
            let logger_ref_cell: &RefCell<TestLogger> = self.logger.borrow();
            logger_ref_cell.borrow_mut().log("Notify done".to_string())
        }
    }
    struct TestLogger {
        messages: Vec<String>,
    }
    impl TestLogger {
        fn log(&mut self, msg: String) {
            self.messages.push(msg)
        }
    }

    #[test]
    fn run_step_reports_success_and_commits() {
        let scripts = vec![
            "..before..".to_string(),
            "..cmd..".to_string(),
            "..after..".to_string(),
        ];
        let mut step_response = StepResponse { sha: None, status: EStatus::Pending, output: None };
        let step_request = StepRequest { run: "cmd".to_string(), run_resolved: scripts.clone(), commit_msg: "..msg..".to_string() };

        // The intent here is is to log is to log all interactions with the  fake objects in one vec.
        // I may have done something silly here to get the compiler to accept it. Better ideas?
        let logger_rc = Rc::new(RefCell::new(TestLogger { messages: vec![] }));
        let mut repo: FakeRepo = FakeRepo {
            logger: logger_rc.clone(),
        };
        let mut executor = FakeExecutor {
            logger: logger_rc.clone(),
            succeed: true,
        };
        let mut notifier = FakeNotifier {
            logger: logger_rc.clone(),
        };
        run_step(
            &mut repo,
            &mut executor,
            &mut notifier,
            1,
            &step_request,
            &mut step_response,
        );
        assert_eq!(step_response.status, EStatus::Done);
        let logger_ref_cell: &RefCell<TestLogger> = logger_rc.borrow();
        insta::assert_yaml_snapshot!(logger_ref_cell.borrow().messages);
        assert_eq!(step_response.sha, Some("..SHA..".to_string()));
    }

    #[test]
    fn run_step_reports_failure_and_resets() {
        let scripts = vec![
            "..before..".to_string(),
            "..cmd..".to_string(),
            "..after..".to_string(),
        ];
        let step_request = StepRequest { run: "cmd".to_string(), run_resolved: scripts.clone(), commit_msg: "..msg..".to_string() };
        let mut step_response = StepResponse { sha: None, status: EStatus::Pending, output: None };

        // The intent here is is to log is to log all interactions with the  fake objects in one vec.
        // I may have done something silly here to get the compiler to accept it. Better ideas?
        let logger_rc = Rc::new(RefCell::new(TestLogger { messages: vec![] }));
        let mut repo: FakeRepo = FakeRepo {
            logger: logger_rc.clone(),
        };
        let mut executor = FakeExecutor {
            logger: logger_rc.clone(),
            succeed: false,
        };
        let mut notifier = FakeNotifier {
            logger: logger_rc.clone(),
        };
        run_step(
            &mut repo,
            &mut executor,
            &mut notifier,
            1,
            &step_request,
            &mut step_response,
        );
        assert_eq!(step_response.status, EStatus::Failed);
        let logger_ref_cell: &RefCell<TestLogger> = logger_rc.borrow();
        insta::assert_yaml_snapshot!(logger_ref_cell.borrow().messages);
        assert_eq!(step_response.sha, None);
    }
}
