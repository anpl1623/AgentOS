//! What the rendering tests share: an uncoloured style, and a run in a
//! given state.

use agentos_core::ids::TaskId;
use agentos_core::task::{TaskRun, TaskState};

use super::Style;

pub(super) fn plain() -> Style {
    Style { enabled: false }
}

pub(super) fn child_run(state: TaskState, steps: u32, tainted: bool) -> TaskRun {
    let mut run = TaskRun::new(TaskId::new(), 1);
    run.state = state;
    run.steps_taken = steps;
    run.tainted = tainted;
    run
}
