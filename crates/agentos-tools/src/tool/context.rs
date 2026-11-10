//! What a tool knows about the run it executes in: [`ToolContext`], with its
//! default budgets, and the [`PolicyProbe`] a tool consults while it runs.

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use agentos_core::ids::{AgentId, TaskId, TaskRunId};
use agentos_core::permission::Capability;

use super::secret::{CredentialLedger, CredentialResolver};

/// Default per-call time budget.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

/// Default cap on how much output a single call may feed back to the model.
///
/// A tool that returns a hundred megabytes of attacker-controlled text is a
/// denial-of-service against the context window, and an expensive one.
pub const DEFAULT_MAX_OUTPUT_BYTES: usize = 64 * 1024;

/// Default cap on how many images a single call may return.
///
/// One capture answers one question. A tool returning a filmstrip is either a
/// mistake or an attempt to fill the context window, and both are worth
/// stopping at the pipeline rather than at the provider.
pub const DEFAULT_MAX_IMAGES: usize = 4;

/// The policy, asked about one capability while a call is already executing.
///
/// Authorisation happens once, against the plan, and a plan names what a call
/// will touch before it touches anything. A tool that discovers what it touches
/// as it goes, such as a recursive search, can only name its root, and path
/// rules match by prefix: a policy that allows a directory and denies a
/// directory inside it authorises the root and says nothing about the inner
/// one. The probe lets such a tool ask the same policy about each path it finds,
/// so a narrower deny binds the walk as well as the plan.
///
/// The pipeline supplies one evaluated with exactly the request shape it used to
/// authorise the call. It answers `true` only for an outright allow: a person
/// who approved the call approved it as planned, not each path it later finds.
pub trait PolicyProbe: Send + Sync + fmt::Debug {
    /// Whether the policy allows `capability`, without asking anyone.
    fn permits(&self, capability: &Capability) -> bool;
}

/// Everything a tool needs to know about the run it is executing inside.
#[derive(Debug, Clone)]
pub struct ToolContext {
    /// The agent.
    pub agent_id: AgentId,
    /// The task.
    pub task_id: TaskId,
    /// The run.
    pub run_id: TaskRunId,
    /// Directory that relative paths are resolved against.
    ///
    /// Being in the workspace does not make a path permitted; the policy decides
    /// that. This only determines what a relative path *means*.
    pub workspace: PathBuf,
    /// Per-call time budget.
    pub timeout: Duration,
    /// Cap on returned output.
    pub max_output_bytes: usize,
    /// Cap on how many images one call may return.
    pub max_images: usize,
    /// Longest edge, in pixels, images are resized to fit within.
    pub max_image_edge: u32,
    /// Cap on the encoded size of each returned image.
    pub max_image_bytes: usize,
    /// The policy, for a tool that must check what it finds during execution.
    ///
    /// `None` outside the pipeline. A tool that depends on it refuses to run
    /// without it rather than fall back to what the plan alone authorised.
    pub policy: Option<Arc<dyn PolicyProbe>>,
    /// Stored credentials, for a tool that authenticates as the operator.
    ///
    /// `None` outside the pipeline and when nothing stores credentials. The
    /// runtime sets the store's own resolver here; the pipeline replaces it,
    /// for each call it executes, with one bound to that call's plan (see
    /// [`CredentialResolver`]), and removes it while the call is planned.
    pub credentials: Option<Arc<dyn CredentialResolver>>,
    /// The capabilities the pipeline authorised this call against, resources
    /// and all.
    ///
    /// Empty outside the pipeline's execute step. A plan can name a resource
    /// it read from the world, such as the origin of the page a browser is on,
    /// and the world can move between the plan and the call. A tool that acts
    /// on such a resource compares what it finds with what was authorised, and
    /// refuses when there is nothing here to compare with.
    pub authorised: Vec<Capability>,
    /// Every credential released during the run this context belongs to.
    ///
    /// Shared by every clone, so one context per run means one ledger per run.
    issued: Arc<CredentialLedger>,
}

impl ToolContext {
    /// A context with default budgets.
    #[must_use]
    pub fn new(agent_id: AgentId, task_id: TaskId, run_id: TaskRunId, workspace: PathBuf) -> Self {
        Self {
            agent_id,
            task_id,
            run_id,
            workspace,
            timeout: DEFAULT_TIMEOUT,
            max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
            max_images: DEFAULT_MAX_IMAGES,
            max_image_edge: crate::vision::DEFAULT_MAX_IMAGE_EDGE,
            max_image_bytes: crate::vision::DEFAULT_MAX_IMAGE_BYTES,
            policy: None,
            credentials: None,
            authorised: Vec::new(),
            issued: Arc::default(),
        }
    }

    /// Attach the policy probe.
    #[must_use]
    pub fn with_policy(mut self, policy: Arc<dyn PolicyProbe>) -> Self {
        self.policy = Some(policy);
        self
    }

    /// Attach the credential store.
    #[must_use]
    pub fn with_credentials(mut self, credentials: Arc<dyn CredentialResolver>) -> Self {
        self.credentials = Some(credentials);
        self
    }

    /// The run's ledger of released credentials.
    pub(crate) fn issued(&self) -> &CredentialLedger {
        &self.issued
    }

    /// The run's ledger, to be written to from outside this context.
    pub(crate) fn issued_handle(&self) -> Arc<CredentialLedger> {
        Arc::clone(&self.issued)
    }

    /// This context with no credential store, for planning.
    pub(crate) fn without_credentials(&self) -> Self {
        Self {
            credentials: None,
            ..self.clone()
        }
    }

    /// Override the time budget.
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Override the output cap.
    #[must_use]
    pub const fn with_max_output_bytes(mut self, max_output_bytes: usize) -> Self {
        self.max_output_bytes = max_output_bytes;
        self
    }

    /// Override the image budgets.
    #[must_use]
    pub const fn with_image_budget(
        mut self,
        max_images: usize,
        max_image_edge: u32,
        max_image_bytes: usize,
    ) -> Self {
        self.max_images = max_images;
        self.max_image_edge = max_image_edge;
        self.max_image_bytes = max_image_bytes;
        self
    }
}
