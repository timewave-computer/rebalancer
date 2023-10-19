use cosmwasm_std::{DecimalRangeExceeded, StdError};
use thiserror::Error;
use valence_package::error::ValenceError;

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error(transparent)]
    ValenceError(#[from] ValenceError),

    #[error(transparent)]
    DecimalRangeExceeded(#[from] DecimalRangeExceeded),

    #[error("Account is already registered")]
    AccountAlreadyRegistered,

    #[error("Base denom is not whitelisted: {0}")]
    BaseDenomNotWhitelisted(String),

    #[error("Denom is not whitelisted: {0}")]
    DenomNotWhitelisted(String),

    #[error("Price is midding for denom: {0}")]
    MissingPriceForDenom(String),

    #[error("The owner already paused this service")]
    AccountAlreadyPaused,

    #[error("This wallet is not authorized to pause this service")]
    NotAuthorizedToPause,

    #[error("This wallet is not authorized to resume this service")]
    NotAuthorizedToResume,

    #[error("Service is not paused")]
    NotPaused,

    #[error("Targets percentage doesn't add to 100%: {0}")]
    InvalidTargetPercentage(String),

    #[error("Can't rebalance, cycle not started yet, next: {0}")]
    CycleNotStartedYet(u64),

    #[error("We got an unexpected reply id: {0}")]
    UnexpectedReplyId(u64),

    #[error("Multiple targets have min_balance, only a single target with min_balance is allowed")]
    MultipleMinBalanceTargets,

    #[error("A minimum of 2 targets are required")]
    TwoTargetsMinimum,

    #[error("Target list must contain only a single instance of a denom")]
    TargetsMustBeUnique,

    #[error("Target with a min_balance wasn't found")]
    NoMinBalanceTargetFound,

    #[error("Account balance is zero")]
    AccountBalanceIsZero,

    #[error("Ivalid rebalance limit, Each base denom must have a limit")]
    InvalidRebalanceLimit,

    #[error("Couldn't find minimum amount for this token in auction manager")]
    NoMinAuctionAmountFound,
}