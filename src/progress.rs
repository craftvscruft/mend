use std::time::{Instant};

use console::Emoji;
use indicatif::{HumanDuration, MultiProgress, ProgressBar, ProgressStyle};

use crate::run::{EStatus, RunStatus};

static SPARKLE: Emoji<'_, '_> = Emoji("✨ ", ":-)");

pub trait Notify {
    fn notify(&self, i: usize, status: &EStatus, msg: String, inc: bool);
    fn notify_done(&self);
}

pub struct ConsoleNotifier {
    started: Instant,
    multi_progress: MultiProgress,
    progress_bars: Vec<ProgressBar>,
}

impl Notify for ConsoleNotifier {
    fn notify(&self, i: usize, status: &EStatus, msg: String, inc: bool) {
        if let Some(progress) = self.progress_bars.get(i) {
            if inc {
                progress.inc(1);
            }
            match status {
                EStatus::Pending => {
                    progress.set_message(msg)
                }
                EStatus::Running => {
                    progress.set_message(msg)
                }
                EStatus::Done => {
                    progress.finish_with_message(msg)
                }
                EStatus::Failed => {
                    progress.abandon_with_message(msg)
                }
            }
        }
    }
    fn notify_done(&self) {
        let _ = self.multi_progress.clear();
        println!("{} Done in {}", SPARKLE, HumanDuration(self.started.elapsed()));
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
        let step_scripts = step_status.run_resolved.len();
        let pb = notifier.multi_progress.add(ProgressBar::new(step_scripts as u64));
        pb.set_style(create_spinner_style());
        pb.set_prefix(format!("[{}/{}]", i + 1, num_steps));
        pb.set_message("Pending");
        notifier.progress_bars.push(pb);
        i += 1
    }
    notifier
}


fn create_spinner_style() -> ProgressStyle {
    ProgressStyle::with_template("{prefix:.bold.dim} {spinner} {wide_msg}")
        .unwrap()
        .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ ")
}