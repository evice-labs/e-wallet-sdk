//! Token facade .
//!
//! Pre-checks and orchestrates token transfers across all four directions:
//! public→public, public→private, private→public, private→private.

use lee::AccountId;

use crate::error::SdkResult;

/// Token transfer direction (for logging and UI display).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferDirection {
    PublicToPublic,
    PublicToPrivate,
    PrivateToPublic,
    PrivateToPrivate,
}

impl std::fmt::Display for TransferDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PublicToPublic => write!(f, "public → public"),
            Self::PublicToPrivate => write!(f, "public → private (shield)"),
            Self::PrivateToPublic => write!(f, "private → public (unshield)"),
            Self::PrivateToPrivate => write!(f, "private → private"),
        }
    }
}

/// Parameters for a token transfer (pre-checked and ready for submission).
#[derive(Debug, Clone)]
pub struct TokenTransferParams {
    pub sender_holding: AccountId,
    pub recipient_holding: AccountId,
    pub token_definition: AccountId,
    pub amount: u128,
    pub direction: TransferDirection,
}

/// Pre-check result before submitting a token transfer.
#[derive(Debug)]
pub struct PreCheckResult {
    pub direction: TransferDirection,
    pub sender_balance: u128,
    pub warnings: Vec<String>,
}

/// Token facade — validates and submits token transfers.
///
/// Before building the transaction, the facade checks:
/// 1. Sender holding decodes as Fungible and its definition_id matches.
/// 2. Sender balance >= amount.
/// 3. Recipient holding exists, is initialised, and has the same definition_id.
///    (Otherwise returns `RecipientNotInitialized`.)
pub struct TokenFacade;

impl TokenFacade {
    /// Pre-check a token transfer.
    ///
    /// Returns the pre-check result or an error if the transfer is invalid.
    /// The actual submission is performed by the engine after approval.
    ///
    /// TODO: Implement actual on-chain state checks via WalletCore.
    pub fn pre_check(
        _sender_holding: &AccountId,
        _recipient_holding: &AccountId,
        _token_definition: &AccountId,
        _amount: u128,
    ) -> SdkResult<PreCheckResult> {
        // TODO: Read sender holding state, decode TokenHolding,
        //           verify definition_id, check balance, read recipient state.
        //
        // For now, return a stub result. The real implementation will call
        // WalletCore::get_account_view() for both holdings and decode them.

        Ok(PreCheckResult {
            direction: TransferDirection::PublicToPublic,
            sender_balance: 0,
            warnings: vec![
                "Token facade pre-checks not yet implemented — verify amounts manually".into(),
            ],
        })
    }
}
