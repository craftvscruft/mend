use std::collections::BTreeMap;
use crate::progress::Notify;
use crate::repo::Repo;
use crate::run::EStatus::{Done, Failed, Running};
use crate::{Mend, Recipe};
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

fn resolve_step_scripts(instruction: &String, mend: &Mend, matching_recipes: BTreeMap<&String, &Recipe>) -> Vec<String> {
    let mut resolved_instruction = "".to_owned();
    let mut scripts = vec![];
    let mut recipe_tags: Vec<String> = vec![];

    for (recipe_name, recipe) in matching_recipes {
        let recipe_fn = format!("function {}() {{\n{}\n}}\n", recipe_name, recipe.run);
        resolved_instruction.push_str(&recipe_fn);
        for tag in &recipe.tags {
            recipe_tags.push(tag.to_string())
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
                |step_text| {
                    let instruction = step_text.to_string();

                    let instruction_trimmed = instruction.trim();
                    let instruction_recipe_name = instruction_trimmed.split_whitespace().next().unwrap_or_default().to_string();
                    let matching_recipes : BTreeMap<&String, &Recipe> = mend.recipes.iter()
                        .filter(|&(recipe_name, _)| (recipe_name.eq(&instruction_recipe_name))).collect();
                    let commit_msg = render_commit_message(instruction_trimmed, &matching_recipes);
                    StepRequest {
                        run: step_text.to_string(),
                        run_resolved: resolve_step_scripts(&instruction, mend, matching_recipes),
                        commit_msg
                    }
                }
            }).collect()
}

fn render_commit_message(instruction: &str, matching_recipes: &BTreeMap<&String, &Recipe>) -> String {
    let commit_template = match matching_recipes.values().next() {
        None => { instruction }
        Some(recipe) => {
            match &recipe.commit_template {
                None => { instruction }
                Some(template) => { template }
            }
        }
    };
    // For now splitting on whitespace, perhaps shlex parse later?
    let args : Vec<&str> = instruction.split_whitespace().collect();
    let context = {
        |s: &_| {
            eprintln!("resolving {}", s);
            if let Ok(arg_num) =  str::parse::<i16>(s) {
                eprintln!("parsed arg_num {}", arg_num);
                if arg_num >= 1 && arg_num < args.len() as i16 {
                    if let Some(found_arg) = args.get(arg_num as usize) {
                        return Some(found_arg.to_string())
                    }
                }
            }
            std::env::var(s).ok()
        }
    };
    let commit_msg = shellexpand::env_with_context_no_errors(&commit_template, context);
    let string = commit_msg.to_string();
    string
}

pub fn run_all_steps<R: Repo, E: Executor, N: Notify>(step_requests: Vec<StepRequest>, notifier: &mut N, worktree_repo: &mut R, executor: &mut E)
    -> Result<(), (StepRequest, StepResponse)>{
    let mut step_i: usize = 0;
    for step_request in step_requests {
        let mut step_response = StepResponse { sha: None, status: EStatus::Pending, output: None };
        run_step(
            worktree_repo,
            executor,
            notifier,
            step_i,
            &step_request,
            &mut step_response,
        );
        step_i += 1;
        if step_response.status == Failed {
            return Err((step_request, step_response))
        }
    }
    return Ok(())
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
        step_response.push_output_str(format!("Committing with message '{}'", step_request.commit_msg).as_str());
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
    use crate::run::{create_run_status_from_mend, EStatus, Executor, run_all_steps, run_command_with_output, run_step, StepRequest, StepResponse};
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
    fn create_run_request_with_recipe_commit_template() {
        let mut mend = create_mend_with_steps(vec!["rename arg1 arg2".to_string()]);

        mend.recipes.insert(
            "rename".to_string(),
            Recipe {
                run: "rename-cli $1 $2".to_string(),
                commit_template: Some("r - Rename $1 to $2".to_string()),
                tag: None,
                tags: vec![],
            },
        );
        let step_requests = create_run_status_from_mend(&mend);
        assert_eq!(step_requests.len(), 1);
        assert_eq!(step_requests.get(0).unwrap().commit_msg, "r - Rename arg1 to arg2");
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
        fn notify_failure(&self, _step_request: &StepRequest, _step_response: &StepResponse) {
            let logger_ref_cell: &RefCell<TestLogger> = self.logger.borrow();
            logger_ref_cell.borrow_mut().log("Notify failure".to_string())
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
        run_step(
            &mut FakeRepo {
                logger: logger_rc.clone(),
            },
            &mut FakeExecutor {
                logger: logger_rc.clone(),
                succeed: false,
            },
            &mut FakeNotifier {
                logger: logger_rc.clone(),
            },
            1,
            &step_request,
            &mut step_response,
        );
        assert_eq!(step_response.status, EStatus::Failed);
        let logger_ref_cell: &RefCell<TestLogger> = logger_rc.borrow();
        insta::assert_yaml_snapshot!(logger_ref_cell.borrow().messages);
        assert_eq!(step_response.sha, None);
    }

    #[test]
    fn run_all_steps_reports_ok_when_steps_pass() {
        let scripts = vec![
            "..before..".to_string(),
            "..cmd..".to_string(),
            "..after..".to_string(),
        ];
        let step_request = StepRequest { run: "cmd".to_string(), run_resolved: scripts.clone(), commit_msg: "..msg..".to_string() };
        let logger_rc = Rc::new(RefCell::new(TestLogger { messages: vec![] }));
        let step_requests = vec![step_request];
        let result = run_all_steps(
            step_requests,
            &mut FakeNotifier {
                logger: logger_rc.clone(),
            },
            &mut FakeRepo {
                logger: logger_rc.clone(),
            },
            &mut FakeExecutor {
                logger: logger_rc.clone(),
                succeed: true,
            }
        );
        assert!(result.is_ok());
    }

    #[test]
    fn run_all_steps_reports_failure_with_failed_step() {
        let scripts = vec![
            "..before..".to_string(),
            "..cmd..".to_string(),
            "..after..".to_string(),
        ];
        let step_request = StepRequest { run: "cmd".to_string(), run_resolved: scripts.clone(), commit_msg: "..msg..".to_string() };
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
        let step_requests = vec![step_request];
        let result = run_all_steps(
            step_requests,
            &mut notifier,
            &mut repo,
            &mut executor
        );
        assert!(result.is_err());
        let (failed_step_request, failed_step_response) = result.err().unwrap();
        assert_eq!(failed_step_request.run, "cmd".to_string());
        assert_eq!(failed_step_response.status, EStatus::Failed);
        let logger_ref_cell: &RefCell<TestLogger> = logger_rc.borrow();
        insta::assert_yaml_snapshot!(logger_ref_cell.borrow().messages);
        assert_eq!(failed_step_response.sha, None);
    }
}