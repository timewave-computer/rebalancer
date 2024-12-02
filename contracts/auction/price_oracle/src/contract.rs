use std::collections::VecDeque;

use auction_package::helpers::{
    approve_admin_change, cancel_admin_change, start_admin_change, verify_admin,
};
use auction_package::states::{ADMIN, PAIRS, PRICES, TWAP_PRICES};
use auction_package::{Pair, Price};
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_json_binary, Binary, Decimal, Deps, DepsMut, Env, MessageInfo, Response, StdResult,
};
use cw2::set_contract_version;
use cw_storage_plus::Bound;
use valence_package::event_indexing::{ValenceEvent, ValenceGenericEvent};

use crate::error::ContractError;
use crate::msg::{ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg};
use crate::state::{Config, PriceStep, ASTRO_PRICE_PATHS, CONFIG, LOCAL_PRICES};

const CONTRACT_NAME: &str = "crates.io:oracle";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

const TWAP_PRICE_MAX_LEN: usize = 10;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    ADMIN.save(deps.storage, &info.sender)?;

    // Set config
    CONFIG.save(
        deps.storage,
        &Config {
            auction_manager_addr: deps.api.addr_validate(&msg.auctions_manager_addr)?,
            seconds_allow_manual_change: msg.seconds_allow_manual_change,
            seconds_auction_prices_fresh: msg.seconds_auction_prices_fresh,
        },
    )?;

    Ok(Response::default().add_attribute("method", "instantiate"))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::UpdatePrice { pair } => {
            pair.verify()?;

            let config = CONFIG.load(deps.storage)?;

            // We get the prices from the auction
            let auction_addr = PAIRS
                .query(
                    &deps.querier,
                    config.auction_manager_addr.clone(),
                    pair.clone(),
                )?
                .ok_or(ContractError::PairAuctionNotFound)?;
            let auction_twap_prices = TWAP_PRICES.query(&deps.querier, auction_addr)?;

            let source;

            // We get last price either form auction or astroport
            let last_price = if can_update_price_from_auction(&config, &env, &auction_twap_prices) {
                source = "auction";
                auction_twap_prices[0].clone()
            } else {
                let steps = ASTRO_PRICE_PATHS
                    .load(deps.storage, pair.clone())
                    .map_err(|_| ContractError::NoAstroPath(pair.clone()))?;
                source = "astroport";
                get_price_from_astroport(deps.as_ref(), &env, steps)?
            };

            let local_prices = update_local_price(deps.branch(), pair.clone(), last_price.clone())?;

            // Calculate the average price
            let avg_price = get_avg_price(local_prices);

            // Save price
            PRICES.save(deps.storage, pair.clone(), &avg_price)?;

            let event = ValenceEvent::OracleUpdatePrice {
                pair: pair.clone(),
                price: avg_price.price,
                source: source.to_string(),
            };

            Ok(Response::default().add_event(event.into()))
        }
        ExecuteMsg::ManualPriceUpdate { pair, price } => {
            let config = CONFIG.load(deps.storage)?;
            verify_admin(deps.as_ref(), &info)?;

            pair.verify()?;

            // sanity check
            if price.is_zero() {
                return Err(ContractError::PriceIsZero);
            }

            // Get the time last update happened
            match PRICES.load(deps.storage, pair.clone()) {
                Ok(Price {
                    time: last_updated, ..
                }) => {
                    // Verify enough time has passed since last update to allow manual update
                    // 'enough time' is defined in the config
                    if env.block.time.seconds()
                        < last_updated.seconds() + config.seconds_allow_manual_change
                    {
                        Err(ContractError::NoTermsForManualUpdate)
                    } else {
                        Ok(())
                    }
                }
                Err(_) => Ok(()),
            }?;

            let price = Price {
                price,
                time: env.block.time,
            };
            let local_prices = update_local_price(deps.branch(), pair.clone(), price.clone())?;

            // Calculate the average price
            let avg_price = get_avg_price(local_prices);

            // Save price
            PRICES.save(
                deps.storage,
                pair.clone(),
                &avg_price,
            )?;
            
            let event = ValenceEvent::OracleUpdatePrice {
                pair,
                price: price.price,
                source: "manual".to_string(),
            };

            Ok(Response::default().add_event(event.into()))
        }
        ExecuteMsg::AddAstroPath { pair, path } => {
            verify_admin(deps.as_ref(), &info)?;

            pair.verify()?;

            if ASTRO_PRICE_PATHS.has(deps.storage, pair.clone()) {
                return Err(ContractError::PricePathAlreadyExists);
            }

            if path.is_empty() {
                return Err(ContractError::PricePathIsEmpty);
            }

            if path[0].denom1 != pair.0 || path[path.len() - 1].denom2 != pair.1 {
                return Err(ContractError::PricePathIsWrong);
            }

            ASTRO_PRICE_PATHS.save(deps.storage, pair.clone(), &path)?;

            let event = ValenceGenericEvent::OracleAddPath { pair, path };

            Ok(Response::default().add_event(event.into()))
        }
        ExecuteMsg::UpdateAstroPath { pair, path } => {
            verify_admin(deps.as_ref(), &info)?;

            pair.verify()?;

            if !ASTRO_PRICE_PATHS.has(deps.storage, pair.clone()) {
                return Err(ContractError::PricePathNotFound);
            }

            if path.is_empty() {
                return Err(ContractError::PricePathIsEmpty);
            }

            if path[0].denom1 != pair.0 || path[path.len() - 1].denom2 != pair.1 {
                return Err(ContractError::PricePathIsWrong);
            }

            ASTRO_PRICE_PATHS.save(deps.storage, pair.clone(), &path)?;

            let event = ValenceGenericEvent::OracleUpdatePath { pair, path };

            Ok(Response::default().add_event(event.into()))
        }
        ExecuteMsg::UpdateConfig {
            auction_manager_addr,
            seconds_allow_manual_change,
            seconds_auction_prices_fresh,
        } => {
            verify_admin(deps.as_ref(), &info)?;

            let mut config = CONFIG.load(deps.storage)?;

            if let Some(auction_manager_addr) = auction_manager_addr {
                config.auction_manager_addr = deps.api.addr_validate(&auction_manager_addr)?;
            }

            if let Some(seconds_allow_manual_change) = seconds_allow_manual_change {
                config.seconds_allow_manual_change = seconds_allow_manual_change;
            }

            if let Some(seconds_auction_prices_fresh) = seconds_auction_prices_fresh {
                config.seconds_auction_prices_fresh = seconds_auction_prices_fresh;
            }

            CONFIG.save(deps.storage, &config)?;

            let event = ValenceGenericEvent::OracleUpdateConfig { config };

            Ok(Response::default().add_event(event.into()))
        }
        ExecuteMsg::StartAdminChange { addr, expiration } => {
            let event = ValenceEvent::OracleStartAdminChange {
                admin: addr.clone(),
            };
            Ok(start_admin_change(deps, &info, &addr, expiration)?.add_event(event.into()))
        }
        ExecuteMsg::CancelAdminChange {} => {
            let event = ValenceEvent::OracleCancelAdminChange {};
            Ok(cancel_admin_change(deps, &info)?.add_event(event.into()))
        }
        ExecuteMsg::ApproveAdminChange {} => {
            let event = ValenceEvent::OracleApproveAdminChange {};
            Ok(approve_admin_change(deps, &env, &info)?.add_event(event.into()))
        }
    }
}

fn can_update_price_from_auction(
    config: &Config,
    env: &Env,
    auction_prices: &VecDeque<Price>,
) -> bool {
    if auction_prices.len() < 3 {
        return false;
    }

    // Make sure last auction ran in the acceptable time frame
    // else we consider the auction prices not up to date
    if auction_prices[0].time.seconds() + config.seconds_auction_prices_fresh
        < env.block.time.seconds()
    {
        return false;
    }

    true
}

fn update_local_price(deps: DepsMut, pair: Pair, price: Price) -> Result<VecDeque<Price>, cosmwasm_std::StdError> {
    // Update the oracle local prices and add last price
    let mut local_prices = match LOCAL_PRICES.load(deps.storage, pair.clone()) {
        Ok(prices) => prices,
        Err(_) => VecDeque::new(),
    };

    // if we have the max amount of prices already, remove the last one first
    if local_prices.len() >= TWAP_PRICE_MAX_LEN {
        local_prices.pop_back();
    }

    // Push the last price into the vector
    local_prices.push_front(price.clone());

    // Save the new list of prices
    LOCAL_PRICES.save(deps.storage, pair.clone(), &local_prices)?;
    Ok(local_prices)
}

fn get_avg_price(vec: VecDeque<Price>) -> Price {
    let (total_count, prices_sum) = vec.iter().fold(
        (Decimal::zero(), Decimal::zero()),
        |(total_count, prices_sum), price| (total_count + Decimal::one(), prices_sum + price.price),
    );

    Price {
        price: prices_sum / total_count,
        time: vec[0].time,
    }
}

fn get_price_from_astroport(
    deps: Deps,
    env: &Env,
    steps: Vec<PriceStep>,
) -> Result<Price, ContractError> {
    let final_denom_amount = steps.iter().fold(
        Decimal::from_atomics(1000000_u128, 0).map_err(ContractError::DecimalRangeExceeded),
        |amount, step| {
            // Build the asset
            let offer_asset = astroport::asset::Asset {
                info: astroport::asset::AssetInfo::NativeToken {
                    denom: step.denom1.clone(),
                },
                amount: amount?.to_uint_floor(),
            };

            let res = astroport::querier::simulate(
                &deps.querier,
                step.pool_address.clone(),
                &offer_asset,
            )?;

            let price = Decimal::from_atomics(
                res.return_amount
                    .checked_add(res.commission_amount)?
                    .checked_add(res.spread_amount)?,
                0,
            )?;
            deps.api.debug(format!("res: {:?}", res).as_str());
            deps.api.debug(format!("Price step: {:?}", price).as_str());

            Ok(price)
        },
    )?;

    let _price = final_denom_amount.checked_div(Decimal::from_atomics(1000000_u128, 0)?)?;
    deps.api.debug(format!("Price: {:?}", _price).as_str());

    let price = Price {
        price: final_denom_amount.checked_div(Decimal::from_atomics(1000000_u128, 0)?)?,
        time: env.block.time,
    };

    Ok(price)
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> Result<Binary, ContractError> {
    match msg {
        QueryMsg::GetPrice { pair } => {
            let price = PRICES.load(deps.storage, pair)?;

            Ok(to_json_binary(&price)?)
        }
        QueryMsg::GetLocalPrice { pair } => {
            let price: Vec<Price> = LOCAL_PRICES.load(deps.storage, pair)?.into();

            Ok(to_json_binary(&price)?)
        }
        QueryMsg::GetAllPrices { from, limit } => {
            let from = from.map(Bound::<Pair>::exclusive);
            let prices = PRICES
                .range(deps.storage, from, None, cosmwasm_std::Order::Ascending)
                .take(limit.unwrap_or(10) as usize)
                .collect::<StdResult<Vec<_>>>()?;

            Ok(to_json_binary(&prices)?)
        }
        QueryMsg::GetConfig => {
            let config = CONFIG.load(deps.storage)?;
            Ok(to_json_binary(&config)?)
        }
        QueryMsg::GetAdmin => Ok(to_json_binary(&ADMIN.load(deps.storage)?)?),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, _env: Env, msg: MigrateMsg) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    match msg {
        MigrateMsg::NoStateChange {} => Ok(Response::default()),
    }
}
