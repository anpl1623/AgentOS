//! One step budget for a whole delegation tree.
//!
//! A run's step budget is what stops a model that never declares itself done
//! from calling the provider forever. Delegation would make that a per-agent
//! promise rather than a per-objective one: a supervisor allowed twenty turns
//! could ask four workers, each allowed twenty more, and spend a hundred on an
//! objective the operator priced at twenty. So the budget belongs to the tree.
//! The root run is granted its agent's `max_steps`, every run it delegates to
//! spends from the same counter, and the tree stops when the counter does.
//!
//! A run that never delegates is the degenerate tree of one. Its budget is
//! granted exactly its agent's `max_steps` and nothing else spends from it, so
//! it stops on exactly the turn it stopped on before the budget was shared.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// The model turns a delegation tree may still take.
#[derive(Debug)]
pub struct RunBudget {
    remaining: AtomicU32,
    granted: u32,
}

impl RunBudget {
    /// A budget of `steps` turns, for the root run of a tree.
    #[must_use]
    pub fn of(steps: u32) -> Arc<Self> {
        Arc::new(Self {
            remaining: AtomicU32::new(steps),
            granted: steps,
        })
    }

    /// Take one turn, if any is left.
    ///
    /// `fetch_update` rather than a load followed by a store, because children
    /// running concurrently spend from the same counter: two of them reading
    /// one remaining turn must not both take it. The same reason
    /// `TaintTracker::observe` uses `swap`.
    pub fn try_spend(&self) -> bool {
        self.remaining
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                left.checked_sub(1)
            })
            .is_ok()
    }

    /// Give back a turn taken by [`Self::try_spend`] that produced no turn.
    ///
    /// A turn is taken before the provider is called, so that two runs cannot
    /// both be promised the last one, and the provider may then fail or be
    /// cancelled. Such an attempt is not a turn: the run record does not count
    /// it, and before the budget was shared it cost nothing. Returning it keeps
    /// the budget equal to the turns the runs record, and a run that never
    /// delegates exactly where it was. Never raises the budget above what was
    /// granted.
    pub fn refund(&self) {
        let _ = self
            .remaining
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                (left < self.granted).then_some(left + 1)
            });
    }

    /// Turns the tree may still take.
    #[must_use]
    pub fn remaining(&self) -> u32 {
        self.remaining.load(Ordering::SeqCst)
    }

    /// Turns the tree was granted when its root run started.
    #[must_use]
    pub const fn granted(&self) -> u32 {
        self.granted
    }

    /// Turns the tree has taken so far.
    #[must_use]
    pub fn spent(&self) -> u32 {
        self.granted.saturating_sub(self.remaining())
    }

    /// The turns a child of `agent_max_steps` can expect to have.
    ///
    /// The lesser of its own agent's limit and what the tree has left. This is
    /// what the child is told about, not a second pool: the child spends from
    /// this same budget, so a sibling running beside it can take turns this
    /// figure counted on.
    #[must_use]
    pub fn child(&self, agent_max_steps: u32) -> u32 {
        agent_max_steps.min(self.remaining())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exhaustion_is_exact() {
        let budget = RunBudget::of(3);
        assert!(budget.try_spend());
        assert!(budget.try_spend());
        assert!(budget.try_spend());
        assert!(!budget.try_spend());
        assert!(!budget.try_spend());
        assert_eq!(budget.remaining(), 0);
        assert_eq!(budget.spent(), 3);
        assert_eq!(budget.granted(), 3);
    }

    #[test]
    fn a_budget_of_nothing_grants_nothing() {
        let budget = RunBudget::of(0);
        assert!(!budget.try_spend());
        assert_eq!(budget.spent(), 0);
    }

    #[test]
    fn concurrent_spenders_succeed_exactly_as_often_as_granted() {
        const GRANTED: u32 = 1_000;
        const THREADS: usize = 8;

        let budget = RunBudget::of(GRANTED);
        let handles: Vec<_> = (0..THREADS)
            .map(|_| {
                let budget = Arc::clone(&budget);
                std::thread::spawn(move || {
                    let mut taken = 0_u32;
                    // Each thread tries for the whole grant, so they contend
                    // for every turn rather than taking turns.
                    for _ in 0..GRANTED {
                        if budget.try_spend() {
                            taken += 1;
                        }
                    }
                    taken
                })
            })
            .collect();

        let taken: u32 = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .sum();
        assert_eq!(taken, GRANTED);
        assert_eq!(budget.remaining(), 0);
    }

    #[test]
    fn a_refund_returns_a_turn_and_never_more_than_was_granted() {
        let budget = RunBudget::of(2);
        assert!(budget.try_spend());
        budget.refund();
        assert_eq!(budget.remaining(), 2);
        budget.refund();
        assert_eq!(budget.remaining(), 2);
        assert_eq!(budget.spent(), 0);
    }

    #[test]
    fn a_child_is_told_the_lesser_of_its_own_limit_and_what_is_left() {
        let budget = RunBudget::of(10);
        for _ in 0..7 {
            assert!(budget.try_spend());
        }
        assert_eq!(budget.child(20), 3);
        assert_eq!(budget.child(2), 2);
    }
}
