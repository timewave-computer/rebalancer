use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Decimal, SubMsg, Uint128};
use serde::Serialize;
use valence_package::{
    event_indexing::ValenceGenericEvent,
    services::rebalancer::{ParsedTarget, RebalancerConfig},
};

pub const TRADE_HARD_LIMIT: Decimal = Decimal::raw(5_u128);

pub(crate) type TradesTuple = (Vec<TargetHelper>, Vec<TargetHelper>);

/// Helper struct for our calculation,
/// it holds the target as well as price, balance, input and the amount we need to trade
#[derive(Debug, Clone)]
pub struct TargetHelper {
    /// our target
    pub target: ParsedTarget,
    /// The price of this denom to base_denom
    /// if this target is a base_denom, the price will be 1
    pub price: Decimal,
    /// The current balance amount of this denom
    pub balance_amount: Uint128,
    /// The current balance value, calculated by balance_amount / price
    pub balance_value: Decimal,
    /// The value we need to trade
    /// can either be to sell or to buy, depends on the calculation
    pub value_to_trade: Decimal,
    /// The minimum value we can send to the auction
    pub auction_min_send_value: Decimal,
}

#[cw_serde]
pub struct RebalanceResponse<E: Serialize> {
    pub config: RebalancerConfig,
    pub msg: Option<SubMsg>,
    pub event: ValenceGenericEvent<E>,
    pub should_pause: bool,
}

impl<E: Serialize> RebalanceResponse<E> {
    pub fn new(
        config: RebalancerConfig,
        msg: Option<SubMsg>,
        event: ValenceGenericEvent<E>,
        should_pause: bool,
    ) -> Self {
        Self {
            config,
            msg,
            event,
            should_pause,
        }
    }
}
