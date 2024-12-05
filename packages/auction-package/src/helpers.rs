use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Decimal, Deps, DepsMut, Env, MessageInfo, Response, Timestamp};
use cw_utils::Expiration;

use crate::{
    error::AuctionError,
    states::{AdminChange, ADMIN, ADMIN_CHANGE},
    Pair, PriceFreshnessStrategy,
};

pub fn verify_admin(deps: Deps, info: &MessageInfo) -> Result<(), AuctionError> {
    if ADMIN.load(deps.storage)? != info.sender {
        return Err(AuctionError::NotAdmin);
    }
    Ok(())
}

#[cw_serde]
pub struct GetPriceResponse {
    pub price: Decimal,
    pub time: Timestamp,
}

#[cw_serde]
pub struct ChainHaltConfig {
    /// Time in seconds of how much of a halt we accept
    pub cap: u128,
    /// seconds each block is generated
    pub block_avg: Decimal,
}

impl Default for ChainHaltConfig {
    fn default() -> Self {
        Self {
            cap: 86000_u128,
            block_avg: Decimal::one() + Decimal::one(),
        }
    }
}

#[cw_serde]
pub struct AuctionConfig {
    pub is_paused: bool,
    pub pair: Pair,
    pub chain_halt_config: ChainHaltConfig,
    pub price_freshness_strategy: PriceFreshnessStrategy,
}

pub fn start_admin_change(
    deps: DepsMut,
    info: &MessageInfo,
    addr: &str,
    expiration: Expiration,
) -> Result<Response, AuctionError> {
    verify_admin(deps.as_ref(), info)?;

    let admin_change = AdminChange {
        addr: deps.api.addr_validate(addr)?,
        expiration,
    };

    ADMIN_CHANGE.save(deps.storage, &admin_change)?;

    Ok(Response::default()
        .add_attribute("new_admin_address", admin_change.addr.to_string())
        .add_attribute("expire", expiration.to_string()))
}

pub fn cancel_admin_change(deps: DepsMut, info: &MessageInfo) -> Result<Response, AuctionError> {
    verify_admin(deps.as_ref(), info)?;

    ADMIN_CHANGE.remove(deps.storage);

    Ok(Response::default())
}

pub fn approve_admin_change(
    deps: DepsMut,
    env: &Env,
    info: &MessageInfo,
) -> Result<Response, AuctionError> {
    let admin_data = ADMIN_CHANGE
        .load(deps.storage)
        .map_err(|_| AuctionError::NoAdminChangeData)?;

    if admin_data.addr != info.sender {
        return Err(AuctionError::NotNewAdmin);
    }

    if admin_data.expiration.is_expired(&env.block) {
        return Err(AuctionError::AdminChangeExpired);
    }

    ADMIN.save(deps.storage, &admin_data.addr)?;
    ADMIN_CHANGE.remove(deps.storage);

    Ok(Response::default())
}
