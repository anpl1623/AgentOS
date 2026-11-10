//! The tool catalogue every client of the runtime is given.
//!
//! Composed here and nowhere else, so that the CLI, the desktop application
//! and the runtime each builds for itself cannot end up offering different
//! tools for the same installation. Composing one has no side effects.

use std::sync::Arc;

use agentos_integrations::account::{AccountDirectory, InMemoryDirectory};
use agentos_tools::ToolRegistry;
use agentos_tools::egress::{AddressPolicy, Egress};

use crate::config::{DelegationLimits, RuntimeConfig};
use crate::delegation::{DelegateTool, DelegationBroker};
use crate::integrations::SqliteAccountDirectory;

/// Build the registry every client gets: the built-in tools, the browser,
/// computer control, the network, the integrations and delegation.
///
/// The composition root owns this so that the CLI and the desktop application
/// cannot end up offering different tools for the same installation — and so
/// that a command listing the catalogue lists the same catalogue an agent is
/// actually given.
///
/// Public and free of side effects: listing the tools should not create a
/// database, launch a browser, or ask macOS for the Accessibility permission.
/// The integrations' accounts are read from the configuration's database the
/// first time a tool asks for one, which listing never does. Its
/// `agent.delegate` belongs to no runtime, so it lists the tool and can
/// perform no delegation; a [`Runtime`] builds its own around its broker.
///
/// [`Runtime`]: crate::Runtime
#[must_use]
pub fn build_registry(config: &RuntimeConfig) -> Arc<ToolRegistry> {
    build_registry_for(
        config,
        Arc::new(SqliteAccountDirectory::at(&config.database_path)),
    )
}

/// The same registry, with the integrations reading `accounts`.
#[must_use]
pub fn build_registry_for(
    config: &RuntimeConfig,
    accounts: Arc<dyn AccountDirectory>,
) -> Arc<ToolRegistry> {
    runtime_registry(config, accounts, unattached_broker(config.delegation))
}

/// What a [`Runtime`] builds for itself, around the database it already has
/// open and the broker it registers its runs with.
///
/// [`Runtime`]: crate::Runtime
pub(crate) fn runtime_registry(
    config: &RuntimeConfig,
    accounts: Arc<dyn AccountDirectory>,
    broker: Arc<DelegationBroker>,
) -> Arc<ToolRegistry> {
    compose_registry_with(
        &Arc::new(agentos_browser::BrowserPool::new(
            agentos_browser::BrowserOptions::new(config.browser_profiles()),
        )),
        accounts,
        broker,
    )
}

/// The same registry again, around a browser pool the caller already holds.
///
/// Only a test needs this: to assert that a run released its browser it has to
/// be looking at the same pool the tools are using, and a second pool would make
/// the assertion pass by being empty. No accounts are bound in it, so an
/// integration tool called through it answers that none is, and its
/// `agent.delegate` refuses every call; [`build_registry_sharing_with`] gives
/// it a runtime's broker.
#[must_use]
pub fn build_registry_sharing(pool: &Arc<agentos_browser::BrowserPool>) -> Arc<ToolRegistry> {
    build_registry_sharing_with(pool, unattached_broker(DelegationLimits::default()))
}

/// [`build_registry_sharing`], with `agent.delegate` performing the
/// delegations of the runtime whose [`Runtime::delegation_broker`] this is.
///
/// [`Runtime::delegation_broker`]: crate::Runtime::delegation_broker
#[must_use]
pub fn build_registry_sharing_with(
    pool: &Arc<agentos_browser::BrowserPool>,
    broker: Arc<DelegationBroker>,
) -> Arc<ToolRegistry> {
    compose_registry_with(pool, Arc::new(InMemoryDirectory::default()), broker)
}

/// The registry itself, around a browser pool and the integrations' accounts.
///
/// As [`compose_registry_with`], with an `agent.delegate` that belongs to no
/// runtime and so refuses every call. A caller composing a registry for a
/// runtime it will run agents on passes that runtime's broker to
/// [`compose_registry_with`] instead.
#[must_use]
pub fn compose_registry(
    pool: &Arc<agentos_browser::BrowserPool>,
    accounts: Arc<dyn AccountDirectory>,
) -> Arc<ToolRegistry> {
    compose_registry_with(
        pool,
        accounts,
        unattached_broker(DelegationLimits::default()),
    )
}

/// The registry itself, around a browser pool, the integrations' accounts and
/// the broker delegations are performed through.
///
/// This is where every tool is registered, the network and the integrations
/// among them, and the only place: a tool added anywhere else is how the CLI
/// and the desktop end up offering different catalogues. A caller composes one
/// directly only to change the browser — the demonstration runs it headed, so
/// that a human can watch — and passes its runtime's
/// [`Runtime::integration_accounts`] and [`Runtime::delegation_broker`], so
/// that nothing else differs.
/// `network.request` is built here with the strict address policy, public
/// addresses only, and nothing a caller passes can loosen it.
///
/// Each integration is registered once, whatever is bound, holding `accounts`
/// and resolving the account it acts as inside each call. So `agentos tools`
/// lists `github.issues.list` whether or not a token is bound; calling it with
/// none bound fails cleanly, naming `agentos integration add github`; and
/// binding a second account later changes nothing registered here. The
/// address policy a GitHub call connects under is the account's, which only
/// the operator sets when binding it; the transport handed over here is
/// strict, and nothing a tool argument says reaches either.
///
/// `agent.delegate` is registered last, and like every other tool is offered
/// to an agent only if the agent lists it and performs nothing its policy
/// does not grant.
///
/// [`Runtime::integration_accounts`]: crate::Runtime::integration_accounts
/// [`Runtime::delegation_broker`]: crate::Runtime::delegation_broker
#[must_use]
pub fn compose_registry_with(
    pool: &Arc<agentos_browser::BrowserPool>,
    accounts: Arc<dyn AccountDirectory>,
    broker: Arc<DelegationBroker>,
) -> Arc<ToolRegistry> {
    let mut registry = agentos_tools::standard_registry();
    for tool in agentos_browser::browser_tools(Arc::clone(pool)) {
        registry.register(tool);
    }
    for tool in agentos_computer::build() {
        registry.register(tool);
    }
    for tool in agentos_tools::network::all() {
        registry.register(tool);
    }
    for tool in agentos_integrations::github::build(Egress::new(AddressPolicy::Strict), accounts) {
        registry.register(tool);
    }
    registry.register(Arc::new(DelegateTool::new(broker)));
    Arc::new(registry)
}

/// A broker no runtime registers runs with, for a registry that lists
/// `agent.delegate` without being able to perform a delegation.
fn unattached_broker(limits: DelegationLimits) -> Arc<DelegationBroker> {
    Arc::new(DelegationBroker::new(limits))
}
