use std::collections::HashSet;

use auction_package::helpers::GetPriceResponse;
use auction_package::Pair;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_json_binary, Binary, Decimal, Deps, DepsMut, Env, Event, MessageInfo, Reply, Response,
    StdResult, Uint128,
};
use cw2::set_contract_version;
use valence_package::error::ValenceError;
use valence_package::helpers::{approve_admin_change, verify_services_manager, OptionalField};
use valence_package::services::rebalancer::{RebalancerExecuteMsg, SystemRebalanceStatus};
use valence_package::states::{ADMIN, SERVICES_MANAGER};

use crate::error::ContractError;
use crate::msg::{InstantiateMsg, ManagersAddrsResponse, QueryMsg, WhitelistsResponse};
use crate::rebalance::execute_system_rebalance;
use crate::state::{
    AUCTIONS_MANAGER_ADDR, BASE_DENOM_WHITELIST, CONFIGS, CYCLE_PERIOD, DENOM_WHITELIST,
    SYSTEM_REBALANCE_STATUS,
};

const CONTRACT_NAME: &str = "crates.io:rebalancer";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

// TODO: Make cycle period configurable
pub const DEFAULT_CYCLE_PERIOD: u64 = 60 * 60 * 24; // 24 hours
/// The default limit of how many accounts we loop over in a single message
/// If wasn't specified in the message
pub const DEFAULT_SYSTEM_LIMIT: u64 = 50;

pub const REPLY_DEFAULT_REBALANCE: u64 = 0;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    // Set the admin
    ADMIN.save(deps.storage, &info.sender)?;

    // verify cycle_start is not too much in the future
    if msg.cycle_start > env.block.time.plus_days(30) {
        return Err(ContractError::CycleStartTooFarInFuture);
    }

    // Save status as not started
    SYSTEM_REBALANCE_STATUS.save(
        deps.storage,
        &SystemRebalanceStatus::NotStarted {
            cycle_start: msg.cycle_start,
        },
    )?;

    // Set the services manager
    SERVICES_MANAGER.save(
        deps.storage,
        &deps.api.addr_validate(&msg.services_manager_addr)?,
    )?;

    // Set our whitelist
    DENOM_WHITELIST.save(deps.storage, &HashSet::from_iter(msg.denom_whitelist))?;
    BASE_DENOM_WHITELIST.save(deps.storage, &HashSet::from_iter(msg.base_denom_whitelist))?;

    // save auction addr
    AUCTIONS_MANAGER_ADDR.save(
        deps.storage,
        &deps.api.addr_validate(&msg.auctions_manager_addr)?,
    )?;

    // Save cycle period time given or the default (24 hours)
    CYCLE_PERIOD.save(
        deps.storage,
        &msg.cycle_period.unwrap_or(DEFAULT_CYCLE_PERIOD),
    )?;

    Ok(Response::default().add_attribute("method", "instantiate"))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: RebalancerExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        RebalancerExecuteMsg::Admin(admin_msg) => admin::handle_msg(deps, env, info, admin_msg),
        RebalancerExecuteMsg::ApproveAdminChange => Ok(approve_admin_change(deps, &env, &info)?),
        RebalancerExecuteMsg::Register { register_for, data } => {
            verify_services_manager(deps.as_ref(), &info)?;
            let data = data.ok_or(ContractError::MustProvideRebalancerData)?;
            let registree = deps.api.addr_validate(&register_for)?;
            let auctions_manager_addr = AUCTIONS_MANAGER_ADDR.load(deps.storage)?;

            if CONFIGS.has(deps.storage, registree.clone()) {
                return Err(ContractError::AccountAlreadyRegistered);
            }

            // Find base denom in our whitelist
            let base_denom_whitelist = BASE_DENOM_WHITELIST
                .load(deps.storage)?
                .into_iter()
                .find(|bd| bd.denom == data.base_denom);

            // If not found error out, because base denom is not whitelisted
            let base_denom = match base_denom_whitelist {
                Some(bd) => Ok(bd),
                None => Err(ContractError::BaseDenomNotWhitelisted(
                    data.base_denom.clone(),
                )),
            }?;

            // Verify we have at least 2 targets
            if data.targets.len() < 2 {
                return Err(ContractError::TwoTargetsMinimum);
            }

            // check target denoms are whitelisted
            let denom_whitelist = DENOM_WHITELIST.load(deps.storage)?;
            let mut total_bps: u64 = 0;
            let mut has_min_balance = false;
            let mut min_value_is_met = false;
            let mut total_value = Uint128::zero();

            for target in data.targets.clone() {
                if !(1..=9999).contains(&target.bps) {
                    return Err(ValenceError::InvalidMaxLimitRange.into());
                }

                total_bps = total_bps
                    .checked_add(target.bps)
                    .ok_or(ContractError::BpsOverflow)?;

                // Verify we only have a single min_balance target
                if target.min_balance.is_some() && has_min_balance {
                    return Err(ContractError::MultipleMinBalanceTargets);
                } else if target.min_balance.is_some() {
                    has_min_balance = true;
                }

                // Verify the target is whitelisted
                if !denom_whitelist.contains(&target.denom) {
                    return Err(ContractError::DenomNotWhitelisted(target.denom));
                }

                // Calculate value of the target and make sure we have the minimum value required
                let curr_balance = deps.querier.query_balance(&registree, &target.denom)?;

                if !min_value_is_met {
                    let value = if target.denom == base_denom.denom {
                        curr_balance.amount
                    } else {
                        let pair = Pair::from((base_denom.denom.clone(), target.denom.clone()));
                        let price = deps
                            .querier
                            .query_wasm_smart::<GetPriceResponse>(
                                auctions_manager_addr.clone(),
                                &auction_package::msgs::AuctionsManagerQueryMsg::GetPrice {
                                    pair: pair.clone(),
                                },
                            )?
                            .price;

                        if price.is_zero() {
                            return Err(ContractError::PairPriceIsZero(pair.0, pair.1));
                        }

                        Decimal::from_atomics(curr_balance.amount, 0)?
                            .checked_div(price)?
                            .to_uint_floor()
                    };

                    total_value = total_value.checked_add(value)?;

                    if total_value >= base_denom.min_balance_limit {
                        min_value_is_met = true;
                    }
                }
            }

            if total_bps != 10000 {
                return Err(ContractError::InvalidTargetPercentage(
                    total_bps.to_string(),
                ));
            }

            // Error if minimum account value is not met
            if !min_value_is_met {
                return Err(ContractError::InvalidAccountMinValue(
                    total_value.to_string(),
                    base_denom.min_balance_limit.to_string(),
                ));
            }

            // save config
            CONFIGS.save(deps.storage, registree, &data.to_config()?)?;

            Ok(Response::default())
        }
        RebalancerExecuteMsg::Deregister { deregister_for } => {
            verify_services_manager(deps.as_ref(), &info)?;
            CONFIGS.remove(deps.storage, deps.api.addr_validate(&deregister_for)?);

            Ok(Response::default())
        }
        RebalancerExecuteMsg::Update { update_for, data } => {
            verify_services_manager(deps.as_ref(), &info)?;
            let account = deps.api.addr_validate(&update_for)?;
            let mut config = CONFIGS.load(deps.storage, account.clone())?;

            if !data.targets.is_empty() {
                let denom_whitelist = DENOM_WHITELIST.load(deps.storage)?;
                let mut total_bps = 0;
                let mut has_min_balance = false;

                for target in data.targets.clone() {
                    total_bps += target.bps;

                    if target.min_balance.is_some() && has_min_balance {
                        return Err(ContractError::MultipleMinBalanceTargets);
                    } else if target.min_balance.is_some() {
                        has_min_balance = true;
                    }

                    if !denom_whitelist.contains(&target.denom) {
                        return Err(ContractError::DenomNotWhitelisted(target.denom));
                    }
                }

                if total_bps != 10000 {
                    return Err(ContractError::InvalidTargetPercentage(
                        Decimal::bps(total_bps).to_string(),
                    ));
                }

                config.has_min_balance = has_min_balance;
                config.targets = data.targets.into_iter().map(|t| t.into()).collect();
            } else {
                // We verify the targets he currently has is still whitelisted
                let denom_whitelist = DENOM_WHITELIST.load(deps.storage)?;

                for target in &config.targets {
                    if !denom_whitelist.contains(&target.denom) {
                        return Err(ContractError::DenomNotWhitelisted(target.denom.to_string()));
                    }
                }
            }

            if let Some(trustee) = data.trustee {
                config.trustee = match trustee {
                    OptionalField::Set(trustee) => {
                        Some(deps.api.addr_validate(&trustee)?.to_string())
                    }
                    OptionalField::Clear => None,
                };
            }

            if let Some(base_denom) = data.base_denom {
                if !BASE_DENOM_WHITELIST
                    .load(deps.storage)?
                    .iter()
                    .any(|bd| bd.denom == base_denom)
                {
                    return Err(ContractError::BaseDenomNotWhitelisted(base_denom));
                }
                config.base_denom = base_denom;
            }

            if let Some(pid) = data.pid {
                config.pid = pid.into_parsed()?;
            }

            if let Some(max_limit) = data.max_limit_bps {
                if !(1..=10000).contains(&max_limit) {
                    return Err(ValenceError::InvalidMaxLimitRange.into());
                }

                config.max_limit = Decimal::bps(max_limit);
            }

            if let Some(target_override_strategy) = data.target_override_strategy {
                config.target_override_strategy = target_override_strategy;
            }

            CONFIGS.save(deps.storage, account, &config)?;

            Ok(Response::default())
        }
        RebalancerExecuteMsg::Pause { pause_for, sender } => {
            verify_services_manager(deps.as_ref(), &info)?;
            let account = deps.api.addr_validate(&pause_for)?;
            let sender = deps.api.addr_validate(&sender)?;

            let mut config = CONFIGS.load(deps.storage, account.clone())?;
            let trustee = config
                .trustee
                .clone()
                .map(|a| deps.api.addr_validate(&a))
                .transpose()?;

            if let Some(pauser) = config.is_paused {
                if let Some(trustee) = trustee {
                    // If we have trustee, and its the pauser, and the sender is the account, we change the pauser to the account
                    // else it means that the pauser is the account, so we error because rebalancer already paused.
                    if pauser == trustee && sender == account {
                        config.is_paused = Some(account.clone());
                    } else {
                        return Err(ContractError::AccountAlreadyPaused);
                    }
                } else {
                    // If we reach here, it means we don't have a trustee, but the rebalancer is paused
                    // which can only mean that the pauser is the account, so we error because rebalancer already paused.
                    return Err(ContractError::AccountAlreadyPaused);
                }
            } else {
                // If we reached here it means the rebalancer is not paused so we check if the sender is valid
                // sender can either be the trustee or the account.
                if sender == account {
                    // If we don't have a trustee, and the sender is the account, then we set him as the pauser
                    config.is_paused = Some(account.clone());
                } else if let Some(trustee) = trustee {
                    // If we have a trustee, and its the sender, then we set him as the pauser
                    if trustee == sender {
                        config.is_paused = Some(trustee);
                    } else {
                        // The sender is not the trustee, so we error
                        return Err(ContractError::NotAuthorizedToPause);
                    }
                } else {
                    // If we reach here, it means we don't have a trustee, and the sender is not the account
                    // so we error because only the account can pause the rebalancer.
                    return Err(ContractError::NotAuthorizedToPause);
                }
            }

            CONFIGS.save(deps.storage, account, &config)?;

            Ok(Response::default())
        }
        RebalancerExecuteMsg::Resume { resume_for, sender } => {
            verify_services_manager(deps.as_ref(), &info)?;
            let account = deps.api.addr_validate(&resume_for)?;
            let sender = deps.api.addr_validate(&sender)?;

            let mut config = CONFIGS.load(deps.storage, account.clone())?;
            let auctions_manager_addr = AUCTIONS_MANAGER_ADDR.load(deps.storage)?;

            // verify minimum balance is met
            let base_denom = BASE_DENOM_WHITELIST
                .load(deps.storage)?
                .iter()
                .find(|bd| bd.denom == config.base_denom)
                .expect("Base denom not found in whitelist")
                .clone();

            let mut total_value = Uint128::zero();
            let mut min_value_met = false;

            for target in &config.targets {
                let target_balance = deps.querier.query_balance(&account, &target.denom)?;

                let value = if target.denom == base_denom.denom {
                    target_balance.amount
                } else {
                    let pair = Pair::from((base_denom.denom.clone(), target.denom.clone()));
                    let price = deps
                        .querier
                        .query_wasm_smart::<GetPriceResponse>(
                            auctions_manager_addr.clone(),
                            &auction_package::msgs::AuctionsManagerQueryMsg::GetPrice {
                                pair: pair.clone(),
                            },
                        )?
                        .price;

                    if price.is_zero() {
                        return Err(ContractError::PairPriceIsZero(pair.0, pair.1));
                    }

                    Decimal::from_atomics(target_balance.amount, 0)?
                        .checked_div(price)?
                        .to_uint_floor()
                };

                total_value = total_value.checked_add(value)?;

                if total_value >= base_denom.min_balance_limit {
                    min_value_met = true;
                }

                if min_value_met {
                    break;
                }
            }

            if !min_value_met {
                return Err(ContractError::InvalidAccountMinValue(
                    total_value.to_string(),
                    base_denom.min_balance_limit.to_string(),
                ));
            }

            let trustee = config
                .trustee
                .clone()
                .map(|a| deps.api.addr_validate(&a))
                .transpose()?;

            // If config is paused
            if let Some(resumer) = config.is_paused {
                // If the sender is the account, we resume
                if sender == account {
                    config.is_paused = None;
                } else if let Some(trustee) = trustee {
                    // If we have a trustee, and its the sender, we resume
                    if sender == trustee && resumer == trustee {
                        config.is_paused = None;
                    } else {
                        // We error because only the account or the trustee can resume
                        return Err(ContractError::NotAuthorizedToResume);
                    }
                } else {
                    // If we don't have a trustee and sender is not account, we error
                    return Err(ContractError::NotAuthorizedToResume);
                }
            } else {
                // config is not paused, so error out
                return Err(ContractError::NotPaused);
            }

            CONFIGS.save(deps.storage, account, &config)?;

            Ok(Response::default())
        }
        RebalancerExecuteMsg::SystemRebalance { limit } => {
            execute_system_rebalance(deps, &env, limit)
        }
    }
}

mod admin {
    use cosmwasm_std::{DepsMut, Env, MessageInfo, Response};
    use valence_package::{
        helpers::{cancel_admin_change, start_admin_change, verify_admin},
        services::rebalancer::{BaseDenom, RebalancerAdminMsg, SystemRebalanceStatus},
        states::SERVICES_MANAGER,
    };

    use crate::{
        error::ContractError,
        state::{
            AUCTIONS_MANAGER_ADDR, BASE_DENOM_WHITELIST, CYCLE_PERIOD, DENOM_WHITELIST,
            SYSTEM_REBALANCE_STATUS,
        },
    };

    pub fn handle_msg(
        deps: DepsMut,
        _env: Env,
        info: MessageInfo,
        msg: RebalancerAdminMsg,
    ) -> Result<Response, ContractError> {
        // Verify that the sender is the admin
        verify_admin(deps.as_ref(), &info)?;

        match msg {
            RebalancerAdminMsg::UpdateSystemStatus { status } => {
                match status {
                    SystemRebalanceStatus::Processing { .. } => {
                        Err(ContractError::CantUpdateStatusToProcessing)
                    }
                    _ => Ok(()),
                }?;

                SYSTEM_REBALANCE_STATUS.save(deps.storage, &status)?;

                Ok(Response::default())
            }
            RebalancerAdminMsg::UpdateDenomWhitelist { to_add, to_remove } => {
                let mut denoms = DENOM_WHITELIST.load(deps.storage)?;

                // first remove denoms
                for denom in to_remove {
                    if !denoms.remove(&denom) {
                        return Err(ContractError::CannotRemoveDenoms(denom));
                    }
                }

                // add new denoms
                denoms.extend(to_add);

                DENOM_WHITELIST.save(deps.storage, &denoms)?;

                Ok(Response::default())
            }
            RebalancerAdminMsg::UpdateBaseDenomWhitelist { to_add, to_remove } => {
                let mut base_denoms = BASE_DENOM_WHITELIST.load(deps.storage)?;

                // first remove denoms
                for denom in to_remove {
                    let bd = BaseDenom::new_empty(&denom);
                    if !base_denoms.remove(&bd) {
                        return Err(ContractError::CannotRemoveBaseDenoms(denom));
                    }
                }

                // add new denoms
                base_denoms.extend(to_add);

                BASE_DENOM_WHITELIST.save(deps.storage, &base_denoms)?;

                Ok(Response::default())
            }
            RebalancerAdminMsg::UpdateServicesManager { addr } => {
                let addr = deps.api.addr_validate(&addr)?;

                SERVICES_MANAGER.save(deps.storage, &addr)?;

                Ok(Response::default())
            }
            RebalancerAdminMsg::UpdateAuctionsManager { addr } => {
                let addr = deps.api.addr_validate(&addr)?;

                AUCTIONS_MANAGER_ADDR.save(deps.storage, &addr)?;

                Ok(Response::default())
            }
            RebalancerAdminMsg::UpdateCyclePeriod { period } => {
                CYCLE_PERIOD.save(deps.storage, &period)?;

                Ok(Response::default())
            }
            RebalancerAdminMsg::StartAdminChange { addr, expiration } => {
                Ok(start_admin_change(deps, &info, &addr, expiration)?)
            }
            RebalancerAdminMsg::CancelAdminChange => Ok(cancel_admin_change(deps, &info)?),
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::GetConfig { addr } => {
            to_json_binary(&CONFIGS.load(deps.storage, deps.api.addr_validate(&addr)?)?)
        }
        QueryMsg::GetSystemStatus {} => {
            to_json_binary(&SYSTEM_REBALANCE_STATUS.load(deps.storage)?)
        }
        QueryMsg::GetWhiteLists => {
            let denom_whitelist = DENOM_WHITELIST.load(deps.storage)?;
            let base_denom_whitelist = BASE_DENOM_WHITELIST.load(deps.storage)?;

            to_json_binary(&WhitelistsResponse {
                denom_whitelist,
                base_denom_whitelist,
            })
        }
        QueryMsg::GetManagersAddrs => {
            let services = SERVICES_MANAGER.load(deps.storage)?;
            let auctions = AUCTIONS_MANAGER_ADDR.load(deps.storage)?;

            to_json_binary(&ManagersAddrsResponse { services, auctions })
        }
        QueryMsg::GetAdmin => to_json_binary(&ADMIN.load(deps.storage)?),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(_deps: DepsMut, _env: Env, msg: Reply) -> Result<Response, ContractError> {
    match msg.id {
        REPLY_DEFAULT_REBALANCE => Ok(Response::default().add_event(
            Event::new("fail-rebalance").add_attribute("error", msg.result.unwrap_err()),
        )),
        _ => Err(ContractError::UnexpectedReplyId(msg.id)),
    }
}
