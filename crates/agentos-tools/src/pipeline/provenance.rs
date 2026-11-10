//! Provenance: whether a call may have brought outside bytes back, and what
//! the taint tracker is told the run read.
//!
//! Judged from the capabilities the policy authorised, never from a tool's
//! account of itself, and failing closed: a capability is presumed to read
//! unless its action is known only to change something.

use agentos_core::event::AgentEvent;
use agentos_core::permission::{Capability, permission_domains};
use agentos_core::trust::DataSource;

use super::ToolPipeline;
use super::authorise::capability_from_tool_name;
use crate::taint::TaintTracker;
use crate::tool::{ToolContext, ToolPlan};

impl ToolPipeline {
    /// Taint the run with the text of a failure the tool itself composed.
    ///
    /// Called only for a call that may read (see [`call_may_read`]): the model
    /// receives that text labelled as the tool's output, and the tracker is
    /// told exactly what the model was given. Refusals the runtime writes — an
    /// unknown tool, a denial, a declined approval, a timeout — are not
    /// observed: they wrap the model's own arguments, which nothing outside the
    /// run wrote. A denial's reason names the plan's resources, which the plan
    /// may have resolved from the world, so the model is given it without them
    /// (see [`model_facing_reason`]).
    ///
    /// [`model_facing_reason`]: super::authorise::model_facing_reason
    pub(super) async fn observe_failure(
        &self,
        context: &ToolContext,
        tool: &str,
        taint: &TaintTracker,
    ) {
        let source = DataSource::Tool {
            tool: tool.to_owned(),
        };
        if taint.observe(&source) {
            self.emit(
                context,
                AgentEvent::TaintRaised {
                    source,
                    tool: tool.to_owned(),
                },
            )
            .await;
        }
    }
}

/// Whether exercising a capability may bring bytes from outside the runtime
/// into the call's result.
///
/// Keyed on the capability rather than the tool, because the capability is what
/// the policy engine authorised and a tool's description of itself is not. It
/// fails closed: the answer is yes unless the capability is one of the actions
/// known only to change something — a write, a click. A domain this list has
/// never heard of reads, so a later integration, plugin or network tool is
/// held to the provenance floor without anybody remembering to add it here.
fn reads_outside_world(capability: &Capability) -> bool {
    let action = capability.action.as_str();
    let only_changes = match capability.domain.as_str() {
        permission_domains::FILESYSTEM => {
            matches!(action, "write" | "delete" | "copy" | "move")
        }
        permission_domains::COMPUTER => {
            matches!(
                action,
                "click" | "type" | "key" | "move" | "scroll" | "drag"
            )
        }
        _ => false,
    };
    !only_changes
}

/// Whether a call may hand the model bytes from outside the runtime.
///
/// Judged from the capability the tool's name stands for — what the operator
/// enabled and what an empty plan is authorised against — and from every
/// capability the call plans or, before it has a plan, declares. One exception:
/// a filesystem tool that does not itself read, such as `filesystem.copy`,
/// reads its source only to write it somewhere else, and its `filesystem.read`
/// is the reading half of that transfer rather than the result.
pub(super) fn call_may_read(tool: &str, capabilities: &[Capability]) -> bool {
    let named = capability_from_tool_name(tool);
    if reads_outside_world(&named) {
        return true;
    }
    let transfers = named.domain == permission_domains::FILESYSTEM;
    capabilities.iter().any(|capability| {
        let transfer_source = transfers
            && capability.domain == permission_domains::FILESYSTEM
            && capability.action == "read";
        reads_outside_world(capability) && !transfer_source
    })
}

/// Where the text of a successful call came from, for the taint tracker.
///
/// The tool's label is believed when it names an external source, since
/// believing that can only make the run stricter. It is not believed when it
/// claims the operator or the runtime wrote bytes that a call that may read
/// went out and fetched: then the call is recorded as the tool's own output,
/// which is externally influenced. A mislabelled read costs an approval prompt,
/// never a silent consequential action.
pub(super) fn provenance(tool: &str, plan: &ToolPlan, declared: &DataSource) -> DataSource {
    if call_may_read(tool, &plan.capabilities) && !declared.is_externally_influenced() {
        DataSource::Tool {
            tool: tool.to_owned(),
        }
    } else {
        declared.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capability(qualified: &str) -> Capability {
        let (domain, action) = qualified.split_once('.').unwrap_or((qualified, "invoke"));
        Capability::new(domain, action)
    }

    fn may_read(tool: &str, plans: &[&str]) -> bool {
        let plans = plans
            .iter()
            .map(|name| capability(name))
            .collect::<Vec<_>>();
        call_may_read(tool, &plans)
    }

    #[test]
    fn the_reading_floor_fails_closed_for_what_it_does_not_know() {
        // Integrations and plugins arrive later; none of them should need an
        // entry here to be held to the floor.
        assert!(may_read("web.fetch", &[]));
        assert!(may_read("email.read", &[]));
        assert!(may_read("plugin", &[]));
        // `inspect` is not in any list, and an empty plan is judged by the
        // tool's name, so the first-party inspector is covered either way.
        assert!(may_read("computer.inspect", &[]));
        assert!(may_read("computer.inspect", &["computer.read"]));
    }

    #[test]
    fn actions_that_only_change_something_do_not_read() {
        for (tool, plans) in [
            ("filesystem.write", &["filesystem.write"][..]),
            ("filesystem.delete", &["filesystem.delete"][..]),
            (
                "filesystem.move",
                &["filesystem.delete", "filesystem.write"][..],
            ),
            ("computer.click", &["computer.click"][..]),
            ("computer.type", &["computer.type"][..]),
        ] {
            assert!(!may_read(tool, plans), "{tool} should not read");
        }
    }

    #[test]
    fn a_copy_reads_its_source_only_to_write_it() {
        assert!(!may_read(
            "filesystem.copy",
            &["filesystem.read", "filesystem.write"]
        ));
        // The transfer exception is about filesystem reads alone: a write that
        // also plans a browser read is reading.
        assert!(may_read(
            "filesystem.write",
            &["filesystem.write", "browser.read"]
        ));
        // And it never excuses a tool whose own name reads.
        assert!(may_read("filesystem.read", &["filesystem.read"]));
        assert!(may_read(
            "terminal.exec",
            &["filesystem.read", "terminal.exec"]
        ));
    }
}
