//! Effect simulation and transaction summaries.
//!
//! Before the user approves a transaction, the engine computes a
//! [`TransactionSummary`] showing per-account balance diffs and
//! state changes. This gives the user a clear picture of what
//! will happen — even for arbitrary program calls.

use lee::AccountId;
use std::collections::BTreeMap;

/// Source of the effect information — tells the user how much to trust it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectSource {
    Decoded,
    DryRun,
    RawInferred,
    Unavailable,
}

/// A single per-account balance diff.
#[derive(Debug, Clone)]
pub struct BalanceDiff {
    pub account: AccountId,
    pub program: AccountId,
    pub delta: i128,
    pub label: String,
}

/// Summary of all effects of a proposed transaction.
#[derive(Debug, Clone)]
pub struct TransactionSummary {
    pub source: EffectSource,
    pub balance_diffs: Vec<BalanceDiff>,
    pub state_changes: BTreeMap<AccountId, Vec<String>>,
    pub estimated_fee: Option<u128>,
    pub gas_limit: u64,
    pub warnings: Vec<String>,
}

/// Effect simulator.
///
/// Takes a proposed request + current on-chain state and produces
/// a [`TransactionSummary`].
pub struct EffectSimulator;

impl EffectSimulator {
    /// Simulate effects for a native transfer.
    pub fn native_transfer(
        from: AccountId,
        to: AccountId,
        amount: u128,
        gas_limit: u64,
    ) -> TransactionSummary {
        let native_program = lee_core::native_token::NATIVE_TOKEN_PROGRAM_ID;
        TransactionSummary {
            source: EffectSource::Decoded,
            balance_diffs: vec![
                BalanceDiff {
                    account: from,
                    program: native_program,
                    delta: -(amount as i128),
                    label: "Native".into(),
                },
                BalanceDiff {
                    account: to,
                    program: native_program,
                    delta: amount as i128,
                    label: "Native".into(),
                },
            ],
            state_changes: BTreeMap::new(),
            estimated_fee: None,
            gas_limit,
            warnings: vec![],
        }
    }

    /// Simulate effects for a token transfer.
    pub fn token_transfer(
        from_holding: AccountId,
        to_holding: AccountId,
        token_program: AccountId,
        amount: u128,
        gas_limit: u64,
    ) -> TransactionSummary {
        TransactionSummary {
            source: EffectSource::Decoded,
            balance_diffs: vec![
                BalanceDiff {
                    account: from_holding,
                    program: token_program,
                    delta: -(amount as i128),
                    label: "Token".into(),
                },
                BalanceDiff {
                    account: to_holding,
                    program: token_program,
                    delta: amount as i128,
                    label: "Token".into(),
                },
            ],
            state_changes: BTreeMap::new(),
            estimated_fee: None,
            gas_limit,
            warnings: vec![],
        }
    }

    /// Fallback for generic calls where no decoder matched.
    pub fn unavailable(gas_limit: u64) -> TransactionSummary {
        TransactionSummary {
            source: EffectSource::Unavailable,
            balance_diffs: vec![],
            state_changes: BTreeMap::new(),
            estimated_fee: None,
            gas_limit,
            warnings: vec![
                "Unable to determine transaction effects. Review the raw calldata carefully."
                    .into(),
            ],
        }
    }
}
