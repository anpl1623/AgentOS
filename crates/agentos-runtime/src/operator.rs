//! What an operator changes, and the record that they changed it.
//!
//! A policy, an agent's existence, whether it may run, the credentials its
//! model is reached with and the ones its runs may spend at a remote service
//! decide what every later run is able to do. A policy widened five minutes
//! before a bad action and narrowed again afterwards is part of the
//! explanation of that action, so each of these changes is written to the
//! audit chain beside the actions it made possible.
//!
//! This is the one layer the CLI and the desktop application both go through
//! for these changes, which is what makes the record unconditional: neither
//! client can make the change without it. The repositories underneath stay
//! thin and decide nothing; a client that writes to them directly is going
//! around the runtime, which no client in this repository does.
//!
//! Each method makes its change and then records it. A change that succeeds
//! and cannot be recorded returns the audit error rather than swallowing it:
//! the change stands, but the operator is told the log did not take it, which
//! is better than a log that silently has a gap.
//!
//! The same holds for what an agent is told before it plans and for what
//! starts work with nobody present: a memory, a schedule, a queued task and the
//! scheduler itself. Each method here makes exactly one record of its kind.
//!
//! An integration account is bound and unbound here too. It is two changes,
//! a credential and the row that points at it, and each is recorded as
//! itself, so the chain shows a token being stored and an account being bound
//! as the two acts they are.

use std::sync::{Arc, PoisonError};
use std::time::Duration;

use agentos_audit::AuditLog;

use agentos_core::Timestamp;
use agentos_core::agent::{Agent, AgentStatus, ModelConfig};
use agentos_core::event::{AgentEvent, Event};
use agentos_core::ids::{AgentId, IntegrationAccountId, MemoryId, ScheduleId, TaskId, TaskRunId};
use agentos_core::memory::{Memory, MemoryKind};
use agentos_core::schedule::{Cadence, Schedule, ScheduleStatus};
use agentos_core::task::{Task, TaskStatus};
use agentos_core::trust::DataSource;
use agentos_integrations::account::Account;
use agentos_permissions::PolicyDocument;
use agentos_persistence::integrations::IntegrationAccount;
use agentos_providers::provider_ids;
use agentos_secrets::provider_key;
use agentos_tools::egress::{Egress, EgressRequest, Method, Url};
use agentos_tools::{CredentialResolver, MIN_REDACTED_FRAGMENT, Secret, ToolContext, ToolError};

use crate::credentials::{SecretStoreResolver, credential_address, index_entry, index_key, listed};
use crate::integrations::{
    BindingTarget, BoundAccount, CheckOutcome, IntegrationCheck, account_of, binding_target,
};
use crate::scheduler::{SchedulerOptions, SchedulerTransition};
use crate::{Runtime, RuntimeError, path_between};

impl Runtime {
    /// Create an agent with a starter policy scoped to its own workspace.
    ///
    /// The starter policy is deliberately close to useless: read-only inside one
    /// directory. Widening it is an explicit act by the operator.
    ///
    /// Records the agent's creation and then its starter policy, so the history
    /// of what the agent was allowed to do starts at version one rather than at
    /// the first edit somebody made to it.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] if the name is taken or the write fails, and
    /// [`RuntimeError::Audit`] if the change could not be recorded.
    pub async fn create_agent(
        &self,
        name: &str,
        instructions: &str,
        model: ModelConfig,
        tools: Vec<String>,
    ) -> Result<Agent, RuntimeError> {
        let agent = Agent::new(name, instructions, model).with_tools(tools);
        self.database.agents().insert(&agent).await?;
        // Recorded before anything else can fail: once the row exists the
        // agent exists, and its history must not begin after its creation.
        self.record_operator(
            agent.id,
            AgentEvent::AgentCreated {
                agent: agent.name.clone(),
                provider: agent.model.provider.clone(),
                model: agent.model.model.clone(),
                tools: agent.enabled_tools.clone(),
            },
        )
        .await?;

        let workspace = self.config.workspace_for(name);
        std::fs::create_dir_all(&workspace).map_err(|source| {
            RuntimeError::io(format!("creating {}", workspace.display()), source)
        })?;

        let policy = agentos_permissions::starter_policy_yaml(&workspace);
        self.install_policy(&agent, &policy).await?;
        Ok(agent)
    }

    /// Replace an agent's policy.
    ///
    /// Refuses a document that does not compile. A stored policy that fails to
    /// load makes every run of the agent deny everything, which is safe but
    /// bewildering; better to say so at the moment of saving. Returns the new
    /// version.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Policy`] if the document does not parse or compile,
    /// [`RuntimeError::Database`] if the agent does not exist or the write
    /// fails, and [`RuntimeError::Audit`] if the change could not be recorded.
    pub async fn set_policy(&self, agent_id: AgentId, document: &str) -> Result<i64, RuntimeError> {
        PolicyDocument::from_yaml(document)?.compile()?;
        let agent = self.database.agents().get(agent_id).await?;
        self.install_policy(&agent, document).await
    }

    /// Allow an agent to be given work, or stop it being given any.
    ///
    /// Recorded even when the agent was already in the state asked for: the
    /// record is of what the operator did, and a reader should not have to
    /// infer an act from its absence.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] if the agent does not exist or the write
    /// fails, and [`RuntimeError::Audit`] if the change could not be recorded.
    pub async fn set_agent_enabled(
        &self,
        agent_id: AgentId,
        enabled: bool,
    ) -> Result<Agent, RuntimeError> {
        let mut agent = self.database.agents().get(agent_id).await?;
        agent.status = if enabled {
            AgentStatus::Enabled
        } else {
            AgentStatus::Disabled
        };
        self.database.agents().update(&agent).await?;
        self.record_operator(
            agent.id,
            AgentEvent::AgentEnabledChanged {
                agent: agent.name.clone(),
                enabled,
            },
        )
        .await?;
        Ok(agent)
    }

    /// Store a credential for a model provider in the runtime's secret store.
    ///
    /// The record names the provider and nothing else.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Rejected`] for a provider the runtime does not know or
    /// an empty key, [`RuntimeError::Secrets`] if the store refuses — on a
    /// machine with no keychain it is read-only — and [`RuntimeError::Audit`]
    /// if the change could not be recorded.
    pub async fn set_provider_key(&self, provider: &str, key: &str) -> Result<(), RuntimeError> {
        known_provider(provider)?;
        let key = key.trim();
        if key.is_empty() {
            return Err(RuntimeError::Rejected("no key was provided".to_owned()));
        }
        self.secrets.set(&provider_key(provider), key)?;
        self.audit
            .record(Event::new(AgentEvent::ProviderKeySet {
                provider: provider.to_owned(),
            }))
            .await?;
        Ok(())
    }

    /// Remove a stored provider credential.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Rejected`] for a provider the runtime does not know,
    /// [`RuntimeError::Secrets`] if the store fails, and
    /// [`RuntimeError::Audit`] if the change could not be recorded.
    pub async fn remove_provider_key(&self, provider: &str) -> Result<(), RuntimeError> {
        known_provider(provider)?;
        self.secrets.delete(&provider_key(provider))?;
        self.audit
            .record(Event::new(AgentEvent::ProviderKeyRemoved {
                provider: provider.to_owned(),
            }))
            .await?;
        Ok(())
    }

    // -- Network credentials ----------------------------------------------

    /// Store a secret bound to one origin, under a name a request can ask for
    /// it by. Returns the origin in the canonical spelling it is bound under,
    /// which is the spelling a run's request is matched against.
    ///
    /// The record names the origin and the credential and nothing else.
    /// Storing under a name already used for that origin replaces the value.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Rejected`] for an origin that is not
    /// `scheme://host[:port]` over http or https, a name that is not a
    /// credential name, or a secret that is empty or holds a control character;
    /// [`RuntimeError::Secrets`] if the store refuses — on a machine with no
    /// keychain it is read-only, and network credentials are not read from the
    /// environment; [`RuntimeError::Database`] if the listing cannot be written;
    /// and [`RuntimeError::Audit`] if the change could not be recorded.
    pub async fn set_network_credential(
        &self,
        origin: &str,
        name: &str,
        secret: &str,
    ) -> Result<String, RuntimeError> {
        let (origin, name, key) = credential_address(origin, name)?;
        let secret = secret.trim();
        if secret.is_empty() {
            return Err(RuntimeError::Rejected("no secret was provided".to_owned()));
        }
        // A credential is sent as a header value, where a line break would
        // start a header of the caller's choosing. Refused here, where the
        // operator can be told, rather than at the moment a run sends it.
        if secret.chars().any(char::is_control) {
            return Err(RuntimeError::Rejected(
                "a credential cannot contain line breaks or other control characters".to_owned(),
            ));
        }

        self.secrets.set(&key, secret)?;
        self.database
            .settings()
            .set(&index_key(&key), &index_entry(&origin, &name))
            .await?;
        self.audit
            .record(Event::new(AgentEvent::CredentialSet {
                origin: origin.clone(),
                name,
            }))
            .await?;
        Ok(origin)
    }

    /// Remove a stored network credential.
    ///
    /// Recorded even when nothing was stored under that origin and name: the
    /// record is of what the operator did.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Rejected`] for an origin or name that could never have
    /// been stored, [`RuntimeError::Secrets`] if the store fails,
    /// [`RuntimeError::Database`] if the listing cannot be updated, and
    /// [`RuntimeError::Audit`] if the change could not be recorded.
    pub async fn remove_network_credential(
        &self,
        origin: &str,
        name: &str,
    ) -> Result<(), RuntimeError> {
        let (origin, name, key) = credential_address(origin, name)?;
        // The value first: if it cannot be removed, the listing goes on
        // saying it is there, which is true.
        self.secrets.delete(&key)?;
        self.database.settings().delete(&index_key(&key)).await?;
        self.audit
            .record(Event::new(AgentEvent::CredentialRemoved { origin, name }))
            .await?;
        Ok(())
    }

    /// Every stored network credential as `(origin, name)`, by origin and then
    /// name. Never a value, nor anything derived from one.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] on failure.
    pub async fn list_network_credentials(&self) -> Result<Vec<(String, String)>, RuntimeError> {
        let mut listed: Vec<(String, String)> = self
            .database
            .settings()
            .all()
            .await?
            .iter()
            .filter_map(listed)
            .collect();
        listed.sort();
        Ok(listed)
    }

    // -- Integrations -------------------------------------------------------

    /// Check a binding as [`Self::bind_integration`] would, before a token is
    /// asked for: the integration, label, host, flag and note as
    /// [`binding_target`] checks them, and then that nothing is stored where
    /// the token would go.
    ///
    /// Two things can already be there. An account bound under the same label
    /// is using the credential the token would replace. A network credential
    /// stored for the same origin under the same name, with no account
    /// behind it, is one the operator stored for `network.request`, and
    /// binding over it would replace it silently and then, on unbinding,
    /// delete it. Both are refused, and the refusal says which.
    ///
    /// # Errors
    ///
    /// As [`binding_target`], and [`RuntimeError::Rejected`] for either
    /// conflict above; [`RuntimeError::Database`] if the accounts cannot be
    /// read.
    pub async fn check_binding(
        &self,
        integration: &str,
        label: &str,
        host: Option<&str>,
        private_network: bool,
        scopes: Option<&str>,
    ) -> Result<BindingTarget, RuntimeError> {
        let target = binding_target(integration, label, host, private_network, scopes)?;
        if self
            .database
            .integrations()
            .find(integration, label)
            .await?
            .is_some()
        {
            return Err(RuntimeError::Rejected(format!(
                "a {integration} account labelled `{label}` is already bound; remove it first"
            )));
        }
        let backed = self
            .list_integrations()
            .await?
            .iter()
            .any(|bound| bound.uses_credential(&target.origin, label));
        let stored = SecretStoreResolver::new(self.secrets.clone())
            .resolve(&target.origin, label)
            .await
            .is_some();
        if stored && !backed {
            return Err(RuntimeError::Rejected(format!(
                "a network credential named `{label}` is already stored for {}, and no account \
                 uses it; remove it with `agentos credential remove` or under Network \
                 credentials, or bind this account under another label",
                target.origin
            )));
        }
        Ok(target)
    }

    /// Bind an account of an integration: its token, then the row naming it.
    ///
    /// Checked first by [`Self::check_binding`], and then the note is checked
    /// against the token itself, so that a token pasted into both fields is
    /// refused before anything is stored. The token is stored through
    /// [`Self::set_network_credential`], for the origin of the account's host
    /// and under its label, and is recorded there as
    /// `operator.credential.set`. The row is written after it and recorded as
    /// `operator.integration.bound`, so a failure between the two leaves a
    /// secret nothing points at, which `agentos credential list` shows,
    /// rather than an account with nothing behind it.
    ///
    /// `private_network` is the only thing that lets an account's requests
    /// reach a private-network address, and this method is the only writer of
    /// it. The host defaults to the integration's own.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Rejected`] for anything [`Self::check_binding`]
    /// refuses, a note holding part of the token, or an empty token;
    /// [`RuntimeError::Secrets`] if the store refuses;
    /// [`RuntimeError::Database`] if the row cannot be written; and
    /// [`RuntimeError::Audit`] if the change could not be recorded.
    pub async fn bind_integration(
        &self,
        integration: &str,
        label: &str,
        host: Option<&str>,
        private_network: bool,
        scopes: Option<&str>,
        token: Secret,
    ) -> Result<IntegrationAccount, RuntimeError> {
        let target = self
            .check_binding(integration, label, host, private_network, scopes)
            .await?;
        if let Some(scopes) = &target.scopes
            && shows_any_of(scopes, token.expose())
        {
            return Err(RuntimeError::Rejected(
                "the scopes note holds part of the token; the token goes in its own field, and \
                 the note is kept in the database and shown on every listing"
                    .to_owned(),
            ));
        }

        self.set_network_credential(&target.origin, label, token.expose())
            .await?;
        // Zeroed now that the store has it, rather than when the call ends.
        drop(token);

        let account = IntegrationAccount::new(
            integration,
            label,
            &target.host,
            private_network,
            target.scopes,
        );
        self.database.integrations().insert(&account).await?;
        self.audit
            .record(Event::new(AgentEvent::IntegrationBound {
                integration: account.integration.clone(),
                label: account.label.clone(),
                host: account.host.clone(),
                private_network: account.private_network,
            }))
            .await?;
        Ok(account)
    }

    /// Unbind an account: the row, then the token behind it.
    ///
    /// Recorded as `operator.integration.unbound` once the row is gone, which
    /// is the moment no call can act as the account. The token is removed
    /// after, through [`Self::remove_network_credential`], which records its
    /// own `operator.credential.removed`; a crash between the two leaves an
    /// orphan secret rather than a row pointing at nothing. It is kept if
    /// another bound account still names the same origin and label.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] if there is no such account or the write
    /// fails, [`RuntimeError::Secrets`] if the token cannot be removed, and
    /// [`RuntimeError::Audit`] if a change could not be recorded. The token is
    /// removed even when the unbinding could not be recorded, and the audit
    /// error is returned after.
    pub async fn unbind_integration(
        &self,
        account_id: IntegrationAccountId,
    ) -> Result<IntegrationAccount, RuntimeError> {
        let account = self.database.integrations().delete(account_id).await?;
        let recorded = self
            .audit
            .record(Event::new(AgentEvent::IntegrationUnbound {
                integration: account.integration.clone(),
                label: account.label.clone(),
                host: account.host.clone(),
                private_network: account.private_network,
            }))
            .await;

        match account_of(&account).origin() {
            Ok(origin) => {
                let shared = self
                    .database
                    .integrations()
                    .list()
                    .await?
                    .iter()
                    .any(|other| {
                        other.label == account.label
                            && account_of(other).origin().is_ok_and(|o| o == origin)
                    });
                if !shared {
                    self.remove_network_credential(&origin, &account.label)
                        .await?;
                }
            }
            // Only a row edited behind the runtime's back has a host that
            // does not read as an origin, and its token, if it has one, is
            // listed by `agentos credential list` for the operator to remove.
            Err(error) => {
                tracing::warn!(%error, "the unbound account's token could not be located");
            }
        }
        recorded?;
        Ok(account)
    }

    /// Every bound account, by integration and then label, with whether a
    /// token is stored behind it.
    ///
    /// Whether one is stored is asked the way a run asks, through the same
    /// resolver, so "present" here means a call would find it.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] on failure.
    pub async fn list_integrations(&self) -> Result<Vec<BoundAccount>, RuntimeError> {
        let resolver = SecretStoreResolver::new(self.secrets.clone());
        let mut bound = Vec::new();
        for account in self.database.integrations().list().await? {
            let origin = account_of(&account).origin().ok();
            let credential_present = match &origin {
                Some(origin) => resolver.resolve(origin, &account.label).await.is_some(),
                None => false,
            };
            bound.push(BoundAccount {
                account,
                origin,
                credential_present,
            });
        }
        bound.sort_by(|a, b| {
            (&a.account.integration, &a.account.label)
                .cmp(&(&b.account.integration, &b.account.label))
        });
        Ok(bound)
    }

    /// Make one authenticated read as an account, `GET {host}/user`, and say
    /// whether the host is the service's API and accepts the token.
    ///
    /// The request leaves through the same egress path, under the same
    /// address policy, as a tool's call as the account would, so a host the
    /// account cannot reach is reported as one here rather than at the first
    /// run. Nothing is sent when no token is stored.
    ///
    /// Sending the token is recorded as a run's spending of it is, as
    /// `network.credential.used`, naming [`CHECK_TOOL`] as what was given it,
    /// before it is released: the host is the operator's to type, and a
    /// mistyped one is somewhere the chain must show the token went.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] if there is no such account, and
    /// [`RuntimeError::Audit`] if the spend could not be recorded, in which
    /// case nothing was sent. Every way the read itself can fail is an
    /// [`IntegrationCheck`], not an error.
    pub async fn test_integration(
        &self,
        account_id: IntegrationAccountId,
    ) -> Result<IntegrationCheck, RuntimeError> {
        let account = account_of(&self.database.integrations().get(account_id).await?);
        self.check_account(&account, Egress::new(account.address_policy()))
            .await
    }

    /// The read behind [`Self::test_integration`], through `egress`.
    pub(crate) async fn check_account(
        &self,
        account: &Account,
        egress: Egress,
    ) -> Result<IntegrationCheck, RuntimeError> {
        let check = |outcome, detail: String| Ok(IntegrationCheck { outcome, detail });
        let (base, credential) = match (account.base_url(), account.credential()) {
            (Ok(base), Ok(credential)) => (base.to_owned(), credential),
            (Err(error), _) | (_, Err(error)) => {
                return check(CheckOutcome::WrongHost, error.to_string());
            }
        };
        let Ok(url) = Url::parse(&format!("{base}/user")) else {
            return check(
                CheckOutcome::WrongHost,
                format!("`{base}/user` is not a URL a request can be made to"),
            );
        };

        let resolver = Arc::new(SecretStoreResolver::new(self.secrets.clone()));
        if resolver
            .resolve(&credential.origin, &credential.name)
            .await
            .is_none()
        {
            return check(
                CheckOutcome::Unauthorised,
                format!(
                    "no token is stored for `{}` at {}, so nothing was sent; remove the account \
                     and bind it again",
                    account.label, credential.origin
                ),
            );
        }

        let release = Arc::new(RecordedRelease {
            store: SecretStoreResolver::new(self.secrets.clone()),
            audit: Arc::clone(&self.audit),
            failure: std::sync::Mutex::new(None),
        });
        let context = ToolContext::new(
            AgentId::new(),
            TaskId::new(),
            TaskRunId::new(),
            self.config.workspace.clone(),
        )
        .with_credentials(release.clone());
        let request = EgressRequest::new(Method::GET, url)
            .with_header("Accept", "application/vnd.github+json")
            .with_header("X-GitHub-Api-Version", "2022-11-28")
            .with_credential(credential)
            .with_timeout(CHECK_TIMEOUT)
            .with_max_bytes(CHECK_MAX_BYTES);

        let outcome = match egress.send(&context, request).await {
            Ok(response) => classify(&base, response.status, response.text()),
            Err(ToolError::Denied { reason }) => IntegrationCheck {
                outcome: CheckOutcome::WrongHost,
                detail: format!(
                    "refused before connecting: {reason}{}",
                    if account.private_network {
                        ""
                    } else {
                        ". An Enterprise server on a private network needs \
                         `--allow-private-network` when it is bound"
                    }
                ),
            },
            Err(error) => IntegrationCheck {
                outcome: CheckOutcome::Unreachable,
                detail: error.to_string(),
            },
        };
        // A token withheld because its spend could not be recorded failed the
        // read as a missing one would; the operator is told the real reason.
        let failure = release
            .failure
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        match failure {
            Some(error) => Err(error),
            None => Ok(outcome),
        }
    }

    // -- Memory -------------------------------------------------------------

    /// Write a memory for an agent, as the operator.
    ///
    /// The source is always [`DataSource::User`], and no caller can say
    /// otherwise. This method can attest only that a person typed the text. A
    /// caller able to name another source could claim `Web` for a note it
    /// wanted distrusted, or — the direction that matters — launder a webpage's
    /// claim into a trusted note by calling it the operator's. Untrusted
    /// memories exist; they are written by the runtime from what a run read,
    /// never through here.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Rejected`] for empty content, [`RuntimeError::Database`]
    /// if the agent does not exist or the write fails, and
    /// [`RuntimeError::Audit`] if the change could not be recorded.
    pub async fn remember(
        &self,
        agent_id: AgentId,
        kind: MemoryKind,
        content: &str,
        confidence: f32,
    ) -> Result<Memory, RuntimeError> {
        let content = non_empty(content, "a memory needs something to remember")?;
        let agent = self.database.agents().get(agent_id).await?;
        let memory =
            Memory::new(agent.id, kind, content, DataSource::User).with_confidence(confidence);
        self.database.memories().insert(&memory).await?;
        self.record_operator(
            agent.id,
            AgentEvent::MemoryRemembered {
                agent: agent.name,
                memory_id: memory.id,
                kind: kind.as_str().to_owned(),
                content: memory.content.clone(),
            },
        )
        .await?;
        Ok(memory)
    }

    /// Rewrite a memory's content and confidence.
    ///
    /// Its source is kept. An operator correcting the wording of something a
    /// webpage claimed has not thereby checked the claim, so a revised
    /// untrusted memory stays untrusted. To vouch for a statement, forget the
    /// old memory and remember the statement.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Rejected`] for empty content, [`RuntimeError::Database`]
    /// if the memory does not exist or the write fails, and
    /// [`RuntimeError::Audit`] if the change could not be recorded.
    pub async fn revise_memory(
        &self,
        id: MemoryId,
        content: &str,
        confidence: f32,
    ) -> Result<Memory, RuntimeError> {
        let content = non_empty(content, "a memory needs something to remember")?;
        let memory = self.database.memories().get(id).await?;
        let agent = self.database.agents().get(memory.agent_id).await?;
        self.database
            .memories()
            .update(id, content, confidence)
            .await?;
        let revised = self.database.memories().get(id).await?;
        self.record_operator(
            agent.id,
            AgentEvent::MemoryRevised {
                agent: agent.name,
                memory_id: id,
                content: revised.content.clone(),
            },
        )
        .await?;
        Ok(revised)
    }

    /// Delete a memory.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] if the memory does not exist or the write
    /// fails, and [`RuntimeError::Audit`] if the change could not be recorded.
    pub async fn forget_memory(&self, id: MemoryId) -> Result<(), RuntimeError> {
        let memory = self.database.memories().get(id).await?;
        let agent = self.database.agents().get(memory.agent_id).await?;
        self.database.memories().delete(id).await?;
        self.record_operator(
            agent.id,
            AgentEvent::MemoryForgotten {
                agent: agent.name,
                memory_id: id,
                kind: memory.kind.as_str().to_owned(),
                content: memory.content,
            },
        )
        .await
    }

    // -- Schedules ----------------------------------------------------------

    /// Create a schedule.
    ///
    /// Nothing fires until a scheduler is running; the schedule is standing
    /// permission for it to start work when one is.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::InvalidSchedule`] if the cadence cannot be evaluated or
    /// the name or objective is empty, [`RuntimeError::Database`] if the agent
    /// does not exist, the name is taken or the write fails, and
    /// [`RuntimeError::Audit`] if the change could not be recorded.
    pub async fn create_schedule(
        &self,
        agent_id: AgentId,
        name: &str,
        objective: &str,
        cadence: Cadence,
        first_run_at: Timestamp,
    ) -> Result<Schedule, RuntimeError> {
        let agent = self.database.agents().get(agent_id).await?;
        let schedule = Schedule::new(agent.id, name, objective, cadence, first_run_at)
            .map_err(|error| RuntimeError::InvalidSchedule(error.to_string()))?;
        self.database.schedules().insert(&schedule).await?;
        self.record_operator(
            agent.id,
            AgentEvent::ScheduleCreated {
                schedule_id: schedule.id,
                name: schedule.name.clone(),
                objective: schedule.objective.clone(),
                cadence: schedule.cadence.clone(),
                next_run_at: schedule.next_run_at,
            },
        )
        .await?;
        Ok(schedule)
    }

    /// Stop a schedule firing without deleting it, or start it again.
    ///
    /// Pausing keeps its next occurrence. Resuming computes the next occurrence
    /// forward from now, so a schedule paused over a weekend does not wake up
    /// owing a backlog; a one-shot whose moment passed while it was paused has
    /// nothing left to do and is finished rather than left active and inert.
    ///
    /// Recorded even when the schedule was already in the state asked for, as
    /// an agent being switched on or off is.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] if the schedule does not exist or the write
    /// fails, and [`RuntimeError::Audit`] if the change could not be recorded.
    pub async fn set_schedule_paused(
        &self,
        id: ScheduleId,
        paused: bool,
    ) -> Result<Schedule, RuntimeError> {
        let schedule = self.database.schedules().get(id).await?;
        let (status, next) = if paused {
            (ScheduleStatus::Paused, schedule.next_run_at)
        } else {
            let now = agentos_core::now();
            let next = match schedule.next_run_at {
                // Its slot is still ahead; nothing was missed.
                Some(next) if next > now => Some(next),
                _ => schedule.cadence.next_after(now),
            };
            let status = if next.is_some() {
                ScheduleStatus::Active
            } else {
                ScheduleStatus::Finished
            };
            (status, next)
        };
        self.database
            .schedules()
            .set_status(id, status, next)
            .await?;

        let payload = if paused {
            AgentEvent::SchedulePaused {
                schedule_id: id,
                name: schedule.name.clone(),
            }
        } else {
            AgentEvent::ScheduleResumed {
                schedule_id: id,
                name: schedule.name.clone(),
                next_run_at: next,
            }
        };
        self.record_operator(schedule.agent_id, payload).await?;
        Ok(self.database.schedules().get(id).await?)
    }

    /// Delete a schedule. The tasks it already created are kept: they are
    /// history, and removing the schedule does not mean the work never
    /// happened.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] if the schedule does not exist or the write
    /// fails, and [`RuntimeError::Audit`] if the change could not be recorded.
    pub async fn delete_schedule(&self, id: ScheduleId) -> Result<(), RuntimeError> {
        let schedule = self.database.schedules().get(id).await?;
        self.database.schedules().delete(id).await?;
        self.record_operator(
            schedule.agent_id,
            AgentEvent::ScheduleDeleted {
                schedule_id: id,
                name: schedule.name,
            },
        )
        .await
    }

    // -- Tasks --------------------------------------------------------------

    /// Create a task, waiting for others to succeed first, not starting before
    /// a given moment, both or neither.
    ///
    /// A task with dependencies is stored `Blocked`. Every dependency is
    /// checked to exist before anything is written, so a graph with one bad
    /// edge does not leave a half-built one behind; a new task cannot close a
    /// cycle, since nothing waits for it yet.
    ///
    /// The task is queued, not run. A client that runs it at once passes it to
    /// [`Runtime::start_task`] or [`Runtime::run_task`]; otherwise it starts
    /// only when a scheduler finds it runnable.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Rejected`] for an empty objective,
    /// [`RuntimeError::InvalidGraph`] if a named dependency does not exist,
    /// [`RuntimeError::Database`] if the agent does not exist or the write
    /// fails, and [`RuntimeError::Audit`] if the change could not be recorded.
    pub async fn create_task(
        &self,
        agent_id: AgentId,
        objective: &str,
        depends_on: &[TaskId],
        scheduled_for: Option<Timestamp>,
    ) -> Result<Task, RuntimeError> {
        if objective.trim().is_empty() {
            return Err(RuntimeError::Rejected(
                "a task needs an objective".to_owned(),
            ));
        }
        let agent = self.database.agents().get(agent_id).await?;
        let mut unique: Vec<TaskId> = Vec::with_capacity(depends_on.len());
        for dependency in depends_on {
            if !unique.contains(dependency) {
                unique.push(*dependency);
            }
        }
        let depends_on = unique;
        for dependency in &depends_on {
            if self.database.tasks().find(*dependency).await?.is_none() {
                return Err(RuntimeError::InvalidGraph(format!(
                    "task {dependency} does not exist, so nothing can wait for it"
                )));
            }
        }

        let mut task = Task::new(agent.id, objective);
        if !depends_on.is_empty() {
            task = task.blocked();
        }
        if let Some(when) = scheduled_for {
            task = task.scheduled_for(when);
        }
        // The row and its edges in one write. A new task cannot close a cycle,
        // since nothing waits for it yet, so `link`'s walk has nothing to find;
        // what matters is that no scheduler sees the task before its edges.
        self.database
            .tasks()
            .insert_waiting(&task, &depends_on)
            .await?;

        self.audit
            .record(
                Event::new(AgentEvent::TaskCreated {
                    objective: task.objective.clone(),
                    depends_on,
                    scheduled_for: task.scheduled_for,
                })
                .for_agent(agent.id)
                .for_task(task.id),
            )
            .await?;
        Ok(task)
    }

    /// Make `task` wait for `depends_on`.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::DependencyCycle`] if the edge would close a cycle,
    /// [`RuntimeError::InvalidGraph`] if a task is missing or the edge is a
    /// self-loop, [`RuntimeError::Database`] on failure, and
    /// [`RuntimeError::Audit`] if the change could not be recorded.
    pub async fn add_task_dependency(
        &self,
        task: TaskId,
        depends_on: TaskId,
    ) -> Result<(), RuntimeError> {
        let waiting = self.link(task, depends_on).await?;
        self.audit
            .record(
                Event::new(AgentEvent::TaskDependencyAdded {
                    task_id: task,
                    depends_on,
                })
                .for_agent(waiting.agent_id)
                .for_task(task),
            )
            .await?;
        Ok(())
    }

    /// Write one edge of a task graph, refusing one that cannot be satisfied.
    ///
    /// Returns the waiting task as it now is. Not recorded here: the operator
    /// act that wrote the edge is, once, by its caller.
    async fn link(&self, task: TaskId, depends_on: TaskId) -> Result<Task, RuntimeError> {
        if task == depends_on {
            return Err(RuntimeError::InvalidGraph(
                "a task cannot wait for itself".to_owned(),
            ));
        }
        for id in [task, depends_on] {
            if self.database.tasks().find(id).await?.is_none() {
                return Err(RuntimeError::InvalidGraph(format!(
                    "task {id} does not exist"
                )));
            }
        }

        // Walk the existing graph from `depends_on`. If `task` is reachable,
        // then `task` is already upstream of `depends_on` and this edge would
        // close the loop.
        let edges = self.database.dependencies().all().await?;
        if let Some(path) = path_between(&edges, depends_on, task) {
            let mut cycle = vec![task];
            cycle.extend(path);
            return Err(RuntimeError::DependencyCycle { path: cycle });
        }

        self.database.dependencies().add(task, depends_on).await?;

        // A task somebody has just made wait is not pending any more.
        let mut stored = self.database.tasks().get(task).await?;
        if matches!(stored.status, TaskStatus::Pending) {
            self.database
                .tasks()
                .set_status(task, TaskStatus::Blocked)
                .await?;
            stored.status = TaskStatus::Blocked;
        }
        Ok(stored)
    }

    // -- Scheduler ----------------------------------------------------------

    /// Record that a scheduler started or stopped, and how it was paced.
    ///
    /// Called by the client that owns the scheduler, after it has taken the
    /// lease and before it ticks, and after it has stopped and drained. Between
    /// the two records, work in this installation started with nobody present.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Audit`] if the change could not be recorded.
    pub async fn record_scheduler_state(
        &self,
        transition: SchedulerTransition,
        options: &SchedulerOptions,
    ) -> Result<(), RuntimeError> {
        let tick_seconds = options.tick.as_secs();
        let max_concurrent_runs = u32::try_from(options.max_concurrent_runs).unwrap_or(u32::MAX);
        let payload = match transition {
            SchedulerTransition::Started { on_launch } => AgentEvent::SchedulerStarted {
                tick_seconds,
                max_concurrent_runs,
                on_launch,
            },
            SchedulerTransition::Stopped => AgentEvent::SchedulerStopped {
                tick_seconds,
                max_concurrent_runs,
            },
        };
        self.audit.record(Event::new(payload)).await?;
        Ok(())
    }

    /// Store a policy that has already been checked, and record it.
    async fn install_policy(&self, agent: &Agent, document: &str) -> Result<i64, RuntimeError> {
        let version = self
            .database
            .agents()
            .set_policy(agent.id, document)
            .await?;
        self.record_operator(
            agent.id,
            AgentEvent::PolicyChanged {
                agent: agent.name.clone(),
                version,
                document: document.to_owned(),
            },
        )
        .await?;
        Ok(version)
    }

    /// Record an operator change about one agent.
    ///
    /// Attached to the agent and to no task or run: nothing was running when
    /// the operator made it, and the record says so.
    async fn record_operator(
        &self,
        agent_id: AgentId,
        payload: AgentEvent,
    ) -> Result<(), RuntimeError> {
        self.audit
            .record(Event::new(payload).for_agent(agent_id))
            .await?;
        Ok(())
    }
}

/// The secret store as an account check spends from it.
///
/// Each release is recorded as `network.credential.used` before the value is
/// handed over, as the pipeline records a run's, and at the same moment: when
/// the egress path asks, after the address has been checked, so a host that
/// is refused shows no spend. A release that cannot be recorded is not made,
/// and the failure is kept for the check to report.
#[derive(Debug)]
struct RecordedRelease {
    store: SecretStoreResolver,
    audit: Arc<AuditLog>,
    failure: std::sync::Mutex<Option<RuntimeError>>,
}

#[async_trait::async_trait]
impl CredentialResolver for RecordedRelease {
    async fn resolve(&self, origin: &str, name: &str) -> Option<Secret> {
        let secret = self.store.resolve(origin, name).await?;
        let recorded = self
            .audit
            .record(Event::new(AgentEvent::CredentialUsed {
                origin: origin.to_owned(),
                name: name.to_owned(),
                tool: CHECK_TOOL.to_owned(),
            }))
            .await;
        match recorded {
            Ok(_) => Some(secret),
            Err(error) => {
                *self.failure.lock().unwrap_or_else(PoisonError::into_inner) = Some(error.into());
                None
            }
        }
    }
}

/// What an account check's spend of the token is recorded as having been
/// given to: the operator's check, not a tool a run could call.
pub const CHECK_TOOL: &str = "operator.integration.test";

/// Whether `text` shows `value`, or any run of [`MIN_REDACTED_FRAGMENT`] bytes
/// of it: what the pipeline would redact, had `value` been released.
fn shows_any_of(text: &str, value: &str) -> bool {
    if value.len() < MIN_REDACTED_FRAGMENT {
        return !value.is_empty() && text.contains(value);
    }
    let text = text.as_bytes();
    value
        .as_bytes()
        .windows(MIN_REDACTED_FRAGMENT)
        .any(|run| text.windows(MIN_REDACTED_FRAGMENT).any(|seen| seen == run))
}

/// How long an account check waits for the host, which is less than a run's
/// request: an operator is watching.
const CHECK_TIMEOUT: Duration = Duration::from_secs(15);

/// How much of the check's answer is read. A user record is a few hundred
/// bytes; anything near this is not one.
const CHECK_MAX_BYTES: usize = 64 * 1024;

/// What a host's answer to `GET {base}/user` says about the account.
///
/// Only the status and, on success, the login are used. The body is never
/// quoted: it is the host's text, and a hostile host can put anything in it,
/// the token it was just sent included.
fn classify(base: &str, status: u16, body: Option<&str>) -> IntegrationCheck {
    let (outcome, detail) = match status {
        200..=299 => match body.and_then(login) {
            Some(login) => (CheckOutcome::Reachable, format!("authenticated as {login}")),
            None => (
                CheckOutcome::WrongHost,
                format!(
                    "the host answered {status}, but not as the GitHub API does; for an \
                     Enterprise server the host ends in `/api/v3`"
                ),
            ),
        },
        300..=399 => (
            CheckOutcome::WrongHost,
            format!(
                "the host redirects ({status}), and redirects are not followed: bind the API's \
                 own address"
            ),
        ),
        401 => (
            CheckOutcome::Unauthorised,
            "the host refused the token (401): it may be mistyped, expired or revoked".to_owned(),
        ),
        403 => (
            CheckOutcome::Unauthorised,
            "the host refused the token (403): it may lack a scope, or the account is rate \
             limited"
                .to_owned(),
        ),
        404 => (
            CheckOutcome::WrongHost,
            format!(
                "`{base}/user` does not exist there (404); for an Enterprise server the host is \
                 usually `https://<server>/api/v3`"
            ),
        ),
        500..=599 => (
            CheckOutcome::Unreachable,
            format!("the host answered {status}: it is up, and not serving the API"),
        ),
        _ => (
            CheckOutcome::WrongHost,
            format!("the host answered {status}, which the GitHub API does not"),
        ),
    };
    IntegrationCheck { outcome, detail }
}

/// The `login` of a user record, if it is one a GitHub account could have:
/// 1 to 39 letters, digits or `-`. Anything else is not reported, since it is
/// not a login and could be whatever the host chose to send.
fn login(body: &str) -> Option<String> {
    let user: serde_json::Value = serde_json::from_str(body).ok()?;
    let login = user.get("login")?.as_str()?;
    (!login.is_empty()
        && login.len() <= 39
        && login
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'))
    .then(|| login.to_owned())
}

/// Operator-typed text with its surrounding whitespace removed, or a refusal
/// saying what was missing.
fn non_empty<'a>(text: &'a str, missing: &str) -> Result<&'a str, RuntimeError> {
    let text = text.trim();
    if text.is_empty() {
        Err(RuntimeError::Rejected(missing.to_owned()))
    } else {
        Ok(text)
    }
}

/// Refuse a provider identifier the runtime cannot build.
///
/// A credential stored under a misspelt provider would never be read, and its
/// record would name a provider that does not exist.
fn known_provider(provider: &str) -> Result<(), RuntimeError> {
    if provider_ids::ALL.contains(&provider) {
        Ok(())
    } else {
        Err(RuntimeError::Rejected(format!(
            "unknown provider `{provider}`; expected one of {}",
            provider_ids::ALL.join(", ")
        )))
    }
}

#[cfg(test)]
mod tests {
    //! The account check against a server on loopback, which only a test can
    //! reach: [`Runtime::test_integration`] builds its transport from the
    //! account alone, and no account admits loopback.

    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::Mutex;

    use agentos_secrets::InMemorySecretStore;

    use super::*;

    const TOKEN: &str = "ghp_EXAMPLETOKEN0123456789abcdefABCDEF";

    /// A loopback server that answers every request with `status` and a body
    /// built from the `Authorization` header it was sent, as a hostile or
    /// careless host might. Returns its origin and every request head.
    fn server(status: &'static str, body: fn(&str) -> String) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let origin = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut head = String::new();
                let mut line = String::new();
                while reader.read_line(&mut line).is_ok_and(|read| read > 2) {
                    head.push_str(&line);
                    line.clear();
                }
                let authorization = head
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("authorization")
                            .then(|| value.trim().to_owned())
                    })
                    .unwrap_or_default();
                log.lock().unwrap().push(head);
                let body = body(&authorization);
                let _ = stream.write_all(
                    format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n\
                         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                );
            }
        });
        (origin, seen)
    }

    async fn check_against(
        status: &'static str,
        body: fn(&str) -> String,
    ) -> (IntegrationCheck, Vec<String>) {
        let guard = tempfile::TempDir::new().unwrap();
        let root = std::fs::canonicalize(guard.path()).unwrap();
        let runtime = Runtime::in_memory(root, Arc::new(InMemorySecretStore::new()))
            .await
            .unwrap();
        let (host, seen) = server(status, body);
        let id = runtime
            .bind_integration(
                "github",
                "work",
                Some(&host),
                false,
                None,
                Secret::new(TOKEN),
            )
            .await
            .unwrap()
            .id;
        let account = account_of(&runtime.database.integrations().get(id).await.unwrap());
        let egress = Egress::for_tests_admitting_loopback(account.address_policy());
        let check = runtime.check_account(&account, egress).await.unwrap();
        assert!(!check.detail.contains("EXAMPLETOKEN"), "{}", check.detail);
        let heads = seen.lock().unwrap().clone();

        // The token was sent, so its spend is in the chain: once per check,
        // naming the check rather than a tool, and never the value.
        let records = runtime.database.audit_sink().all().await.unwrap();
        let spent: Vec<_> = records
            .iter()
            .filter(|record| record.kind == "network.credential.used")
            .collect();
        assert_eq!(spent.len(), heads.len(), "one record per token sent");
        for record in &spent {
            assert_eq!(record.payload["tool"], CHECK_TOOL);
            assert_eq!(record.payload["name"], "work");
            assert_eq!(record.payload["origin"], host.as_str());
        }
        let chain: String = records
            .iter()
            .map(|record| record.payload.to_string())
            .collect();
        assert!(!chain.contains("EXAMPLETOKEN"), "{chain}");
        assert!(runtime.verify_audit().await.unwrap().is_intact());
        (check, heads)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_host_that_knows_the_token_is_reachable_and_was_asked_once() {
        let (check, heads) =
            check_against("200 OK", |_| r#"{"login":"octocat","id":1}"#.to_owned()).await;
        assert_eq!(check.outcome, CheckOutcome::Reachable);
        assert_eq!(check.detail, "authenticated as octocat");
        assert_eq!(heads.len(), 1, "one read, no retry");
        assert!(heads[0].starts_with("GET /user "), "{}", heads[0]);
        assert!(
            heads[0].contains(&format!("Bearer {TOKEN}")),
            "{}",
            heads[0]
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_refusal_that_echoes_the_token_is_reported_without_it() {
        let echo =
            |authorization: &str| format!(r#"{{"message":"Bad credentials: {authorization}"}}"#);
        let (check, _) = check_against("401 Unauthorized", echo).await;
        assert_eq!(check.outcome, CheckOutcome::Unauthorised);
        let (check, _) = check_against("403 Forbidden", echo).await;
        assert_eq!(check.outcome, CheckOutcome::Unauthorised);
        let (check, _) = check_against("500 Internal Server Error", echo).await;
        assert_eq!(check.outcome, CheckOutcome::Unreachable);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn something_other_than_the_api_is_the_wrong_host() {
        let (check, _) = check_against("404 Not Found", |_| "{}".to_owned()).await;
        assert_eq!(check.outcome, CheckOutcome::WrongHost);
        assert!(check.detail.contains("/api/v3"), "{}", check.detail);
        // A success that is not a user record, such as a web page.
        let (check, _) = check_against("200 OK", |_| "<html></html>".to_owned()).await;
        assert_eq!(check.outcome, CheckOutcome::WrongHost);
        // A "login" that is not one is not repeated, whatever it holds.
        let (check, _) = check_against("200 OK", |authorization| {
            format!(
                r#"{{"login":"{}"}}"#,
                authorization.trim_start_matches("Bearer ")
            )
        })
        .await;
        assert_eq!(check.outcome, CheckOutcome::WrongHost);
        let (check, _) = check_against("301 Moved Permanently", |_| String::new()).await;
        assert_eq!(check.outcome, CheckOutcome::WrongHost);
    }
}
