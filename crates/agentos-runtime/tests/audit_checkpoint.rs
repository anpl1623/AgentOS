//! Verifying the audit chain from where the last verification stopped.
//!
//! A screen that polls the chain's health cannot rehash the whole log each
//! time. What it does instead has to prove the same thing about the new
//! records: each is intact, they follow one another, and the first of them
//! extends the part already proved.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use agentos_audit::ChainBreak;
use agentos_core::event::AgentEvent;
use agentos_runtime::{AuditCheckpoint, Runtime};
use agentos_secrets::InMemorySecretStore;
use tempfile::TempDir;

async fn runtime_with(records: usize) -> (Runtime, TempDir) {
    let guard = TempDir::new().unwrap();
    let root = std::fs::canonicalize(guard.path()).unwrap();
    let runtime = Runtime::in_memory(root, Arc::new(InMemorySecretStore::new()))
        .await
        .unwrap();
    append(&runtime, records).await;
    (runtime, guard)
}

async fn append(runtime: &Runtime, count: usize) {
    for _ in 0..count {
        runtime
            .audit()
            .record_payload(AgentEvent::ProviderKeyRemoved {
                provider: "mock".into(),
            })
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn a_first_verification_covers_the_whole_chain() {
    let (runtime, _guard) = runtime_with(3).await;
    let (verification, checkpoint) = runtime.verify_audit_from(None).await.unwrap();
    assert!(verification.is_intact(), "{:?}", verification.breaks);
    assert_eq!(verification.records_checked, 3);
    assert_eq!(checkpoint.sequence, 3);
    assert_eq!(checkpoint.hash, runtime.audit().tip().await.1);
}

#[tokio::test]
async fn a_later_verification_checks_only_what_was_added() {
    let (runtime, _guard) = runtime_with(3).await;
    let (_, checkpoint) = runtime.verify_audit_from(None).await.unwrap();

    let (nothing_new, unchanged) = runtime.verify_audit_from(Some(&checkpoint)).await.unwrap();
    assert!(nothing_new.is_intact());
    assert_eq!(nothing_new.records_checked, 0);
    assert_eq!(unchanged, checkpoint);

    append(&runtime, 2).await;
    let (verification, advanced) = runtime.verify_audit_from(Some(&checkpoint)).await.unwrap();
    assert!(verification.is_intact(), "{:?}", verification.breaks);
    assert_eq!(verification.records_checked, 2);
    assert_eq!(advanced.sequence, 5);
}

#[tokio::test]
async fn a_stretch_that_does_not_extend_the_checkpoint_is_a_break() {
    // The first new record must name the checkpoint's hash as its
    // predecessor. A checkpoint that disagrees with the log — the log was
    // rewritten behind it, or the checkpoint was — is reported, and the
    // checkpoint is not advanced past it.
    let (runtime, _guard) = runtime_with(2).await;
    let forged = AuditCheckpoint {
        sequence: 1,
        hash: "0".repeat(64),
    };
    let (verification, next) = runtime.verify_audit_from(Some(&forged)).await.unwrap();
    assert!(!verification.is_intact());
    assert!(
        verification.breaks.iter().any(|b| matches!(
            b,
            ChainBreak::BrokenLink {
                sequence: 2,
                expected_sequence: 1
            }
        )),
        "{:?}",
        verification.breaks
    );
    assert_eq!(next, forged);
}

#[tokio::test]
async fn a_checkpoint_before_the_first_record_must_be_genesis() {
    let (runtime, _guard) = runtime_with(2).await;
    let (_, whole) = runtime.verify_audit_from(None).await.unwrap();
    let misplaced = AuditCheckpoint {
        sequence: 0,
        hash: whole.hash,
    };
    let (verification, _) = runtime.verify_audit_from(Some(&misplaced)).await.unwrap();
    assert!(
        verification.breaks.contains(&ChainBreak::BadGenesis),
        "{:?}",
        verification.breaks
    );
}
