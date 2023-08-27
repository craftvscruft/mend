use std::time::Instant;

use console::{Emoji, Style};
use indicatif::{HumanDuration, MultiProgress, ProgressBar, ProgressStyle};

use crate::run::{EStatus, RunStatus, StepStatus};

static SPARKLE: Emoji<'_, '_> = Emoji("✨ ", ":-)");

pub trait Notify {
    fn notify(&self, i: usize, step_status: &StepStatus, inc: bool);
    fn notify_done(&self);
}

pub struct ConsoleNotifier {
    started: Instant,
    multi_progress: MultiProgress,
    progress_bars: Vec<ProgressBar>,
}

impl Notify for ConsoleNotifier {
    fn notify(&self, i: usize, step_status: &StepStatus, inc: bool) {
        if let Some(progress) = self.progress_bars.get(i) {
            if inc {
                progress.inc(1);
            }
            let msg = step_status.run.to_string();
            let dim_style: Style = Style::new().dim();
            match step_status.status {
                EStatus::Pending => {
                    let pending_style: Style = Style::new().dim();
                    let styled_status = pending_style.apply_to("Pending");
                    progress.set_message(format!("{} {}", styled_status, dim_style.apply_to(msg)))
                }
                EStatus::Running => {
                    let running_style: Style = Style::new().blue().bold();
                    let styled_status = running_style.apply_to("Running");
                    progress.set_message(format!("{} {}", styled_status, msg))
                }
                EStatus::Done => {
                    let done_style: Style = Style::new().green();
                    let styled_status = done_style.apply_to("Done   ");
                    progress.set_message(format!("{} {}", styled_status, dim_style.apply_to(msg)));
                    progress.finish()
                }
                EStatus::Failed => {
                    let failed_style: Style = Style::new().red().bold();
                    let styled_status = failed_style.apply_to("Failed ");
                    progress.set_message(format!("{} {}", styled_status, msg));
                    progress.abandon()
                }
            }
        }
    }
    fn notify_done(&self) {
        // let _ = self.multi_progress.clear();
        println!(
            "{} Done in {}",
            SPARKLE,
            HumanDuration(self.started.elapsed())
        );
    }
}

pub fn create_console_notifier(run_status: &RunStatus) -> ConsoleNotifier {
    let mut notifier = ConsoleNotifier {
        started: Instant::now(),
        multi_progress: MultiProgress::new(),
        progress_bars: vec![],
    };
    let mut i = 0;
    let num_steps = run_status.steps.len();
    for step_status in run_status.steps.as_slice() {
        let num_step_scripts = step_status.run_resolved.len() + 1;
        let pb = notifier
            .multi_progress
            .add(ProgressBar::new(num_step_scripts as u64));
        pb.set_style(create_spinner_style());
        let i_padding = if i < 10 && num_steps >= 10 { " " } else { "" };
        pb.set_prefix(format!("[{}]{}", i + 1, i_padding));
        // pb.set_prefix(format!("[{}/{}]", i + 1, num_steps));
        notifier.progress_bars.push(pb);
        notifier.notify(i.clone(), step_status, false);
        i += 1
    }
    notifier
}

fn create_spinner_style() -> ProgressStyle {
    ProgressStyle::with_template("{spinner} {prefix:.bold.dim} {wide_msg}").unwrap()
    // .progress_chars("##-")
    // .tick_chars("-| ")
}
