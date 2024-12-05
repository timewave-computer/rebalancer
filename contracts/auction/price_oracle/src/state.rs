use std::collections::VecDeque;

use auction_package::{Pair, Price};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::Addr;
use cw_storage_plus::{Item, Map};

pub const CONFIG: Item<Config> = Item::new("config");
pub const ASTRO_PRICE_PATHS: Map<Pair, Vec<PriceStep>> = Map::new("astro_price_paths");
/// Local last 10 prices to be calculated for the average
pub const LOCAL_PRICES: Map<Pair, VecDeque<Price>> = Map::new("local_prices");

#[cw_serde]
pub struct Config {
    /// The address of the auctions manager contract
    pub auction_manager_addr: Addr,
    /// If the price wasn't changed for this amount of time, the admin can change the price manually
    pub seconds_allow_manual_change: u64,
    /// The amount of seconds we use auctions as our price source
    /// If last auction ran more than this amount of seconds, we do not use the auction as the source of price
    pub seconds_auction_prices_fresh: u64,
}

#[cw_serde]
pub struct PriceStep {
    pub denom1: String,
    pub denom2: String,
    pub pool_address: Addr,
}
