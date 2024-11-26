use std::collections::HashSet;

use auction_package::helpers::GetPriceResponse;
use auction_package::Pair;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_json_binary, Addr, Binary, Coin, Decimal, Deps, DepsMut, Env, Event, MessageInfo, Reply,
    Response, SignedDecimal, StdError, StdResult, Uint128,
};
use cw2::set_contract_version;
use cw_storage_plus::Bound;
use valence_package::error::ValenceError;
use valence_package::event_indexing::ValenceEvent;
use valence_package::helpers::{approve_admin_change, verify_services_manager, OptionalField};
use valence_package::services::rebalancer::{
    PauseData, RebalancerExecuteMsg, SystemRebalanceStatus,
};
use valence_package::states::{QueryFeeAction, ADMIN, SERVICES_MANAGER, SERVICE_FEE_CONFIG};

use crate::error::ContractError;
use crate::msg::{InstantiateMsg, ManagersAddrsResponse, MigrateMsg, QueryMsg, WhitelistsResponse};
use crate::rebalance::execute_system_rebalance;
use crate::state::{
    AUCTIONS_MANAGER_ADDR, BASE_DENOM_WHITELIST, CONFIGS, CYCLE_PERIOD, DENOM_WHITELIST,
    PAUSED_CONFIGS, SYSTEM_REBALANCE_STATUS,
};

const CONTRACT_NAME: &str = "crates.io:rebalancer";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

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

    // store the fees
    SERVICE_FEE_CONFIG.save(deps.storage, &msg.fees)?;

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
        RebalancerExecuteMsg::ApproveAdminChange {} => {
            let event = ValenceEvent::RebalancerApproveAdminChange {};
            Ok(approve_admin_change(deps, &env, &info)?.add_event(event.into()))
        }
        RebalancerExecuteMsg::Register { register_for, data } => {
            let manager_addr = verify_services_manager(deps.as_ref(), &info)?;
            let data = data.ok_or(ContractError::MustProvideRebalancerData)?;
            let registree = deps.api.addr_validate(&register_for)?;

            if CONFIGS.has(deps.storage, registree.clone()) {
                return Err(ContractError::AccountAlreadyRegistered);
            }

            // Verify user paid the registration fee
            let fee_msg = SERVICE_FEE_CONFIG
                .load(deps.storage)?
                .handle_registration_fee(&info, &manager_addr)?;

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

            let auctions_manager_addr = AUCTIONS_MANAGER_ADDR.load(deps.storage)?;
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
            let config = data.to_config(deps.api)?;
            CONFIGS.save(deps.storage, registree.clone(), &config)?;

            let event = ValenceEvent::RebalancerRegister {
                account: registree.to_string(),
                config,
            };

            Ok(Response::default()
                .add_event(event.into())
                .add_messages(fee_msg))
        }
        RebalancerExecuteMsg::Deregister { deregister_for } => {
            verify_services_manager(deps.as_ref(), &info)?;
            let account = deps.api.addr_validate(&deregister_for)?;

            CONFIGS.remove(deps.storage, account.clone());
            PAUSED_CONFIGS.remove(deps.storage, account.clone());

            let event = ValenceEvent::RebalancerDeregister {
                account: account.to_string(),
            };

            Ok(Response::default().add_event(event.into()))
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
                    OptionalField::Set(trustee) => Some(deps.api.addr_validate(&trustee)?),
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

                // If PID is updated, we reset the last calculation because they are no longer valid
                config.targets.iter_mut().for_each(|t| {
                    t.last_input = None;
                    t.last_i = SignedDecimal::zero();
                });
            }

            if let Some(max_limit_option) = data.max_limit_bps {
                config.max_limit = match max_limit_option {
                    OptionalField::Set(max_limit) => {
                        if !(1..=10000).contains(&max_limit) {
                            return Err(ValenceError::InvalidMaxLimitRange.into());
                        }
                        Decimal::bps(max_limit)
                    }
                    OptionalField::Clear => Decimal::one(),
                };
            }

            if let Some(target_override_strategy) = data.target_override_strategy {
                config.target_override_strategy = target_override_strategy;
            }

            CONFIGS.save(deps.storage, account.clone(), &config)?;

            let event = ValenceEvent::RebalancerUpdate {
                account: account.to_string(),
                config,
            };

            Ok(Response::default().add_event(event.into()))
        }
        RebalancerExecuteMsg::Pause {
            pause_for,
            sender,
            reason,
        } => {
            verify_services_manager(deps.as_ref(), &info)?;
            let account = deps.api.addr_validate(&pause_for)?;
            let sender = deps.api.addr_validate(&sender)?;

            if let Some(mut paused_data) = PAUSED_CONFIGS.may_load(deps.storage, account.clone())? {
                // If the sender already paused it before, just error out.
                if sender == paused_data.pauser || account == paused_data.pauser {
                    return Err(ContractError::AccountAlreadyPaused);
                }

                // If the sender is the account, we set it as the pauser to override other possible pausers
                if sender == account {
                    paused_data.pauser = account.clone();

                    PAUSED_CONFIGS.save(deps.storage, account, &paused_data)?;
                    return Ok(Response::default());
                }

                // If we have trustee, and he is the sender we set him as the pauser
                if let Some(trustee) = paused_data.config.trustee.clone() {
                    if sender == trustee {
                        paused_data.pauser = account.clone();

                        PAUSED_CONFIGS.save(deps.storage, account, &paused_data)?;
                        return Ok(Response::default());
                    }
                }

                return Err(ContractError::NotAuthorizedToPause);
            }

            let config = CONFIGS.load(deps.storage, account.clone())?;

            let mut move_config_to_paused = |pauser: Addr| -> Result<(), StdError> {
                CONFIGS.remove(deps.storage, account.clone());
                PAUSED_CONFIGS.save(
                    deps.storage,
                    account.clone(),
                    &PauseData::new(pauser, reason.clone().unwrap_or_default(), &config),
                )?;
                Ok(())
            };

            let response: Response = {
                if sender == account {
                    move_config_to_paused(account.clone())?;
                    return Ok(Response::default());
                };

                if let Some(trustee) = config.trustee.clone() {
                    if trustee == sender {
                        move_config_to_paused(trustee.clone())?;
                        return Ok(Response::default());
                    }
                }

                Err(ContractError::NotAuthorizedToPause)
            }?;

            let event = ValenceEvent::RebalancerPause {
                account: account.to_string(),
                reason: reason.unwrap_or_default(),
            };

            Ok(response.add_event(event.into()))
        }
        RebalancerExecuteMsg::Resume { resume_for, sender } => {
            let manager_addr = verify_services_manager(deps.as_ref(), &info)?;
            let account = deps.api.addr_validate(&resume_for)?;
            let sender = deps.api.addr_validate(&sender)?;

            let paused_data = PAUSED_CONFIGS
                .load(deps.storage, account.clone())
                .map_err(|_| ContractError::NotPaused)?;
            let auctions_manager_addr = AUCTIONS_MANAGER_ADDR.load(deps.storage)?;

            // Verify user paid the resume fee if its needed
            let fee_msg = SERVICE_FEE_CONFIG.load(deps.storage)?.handle_resume_fee(
                &info,
                &manager_addr,
                paused_data.reason,
            )?;

            // Verify sender is autorized to resume
            (|| {
                if sender == account {
                    return Ok(());
                }

                if let Some(trustee) = paused_data.config.trustee.clone() {
                    if sender == trustee && paused_data.pauser != account {
                        return Ok(());
                    }
                }

                Err(ContractError::NotAuthorizedToResume)
            })()?;

            // verify minimum balance is met
            let base_denom = BASE_DENOM_WHITELIST
                .load(deps.storage)?
                .iter()
                .find(|bd| bd.denom == paused_data.config.base_denom)
                .expect("Base denom not found in whitelist")
                .clone();

            let mut total_value = Uint128::zero();
            let mut min_value_met = false;

            for target in &paused_data.config.targets {
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

            CONFIGS.save(deps.storage, account.clone(), &paused_data.config)?;
            PAUSED_CONFIGS.remove(deps.storage, account.clone());

            let event = ValenceEvent::RebalancerResume {
                account: account.to_string(),
            };

            Ok(Response::default()
                .add_event(event.into())
                .add_messages(fee_msg))
        }
        RebalancerExecuteMsg::SystemRebalance { limit } => {
            execute_system_rebalance(deps, &env, limit)
        }
    }
}

mod admin {
    use cosmwasm_std::{DepsMut, Env, MessageInfo, Response};
    use valence_package::{
        event_indexing::ValenceEvent,
        helpers::{cancel_admin_change, start_admin_change, verify_admin},
        services::rebalancer::{BaseDenom, RebalancerAdminMsg, SystemRebalanceStatus},
        states::{SERVICES_MANAGER, SERVICE_FEE_CONFIG},
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

                let event = ValenceEvent::RebalancerUpdateSystemStatus { status };

                Ok(Response::default().add_event(event.into()))
            }
            RebalancerAdminMsg::UpdateDenomWhitelist { to_add, to_remove } => {
                let mut denoms = DENOM_WHITELIST.load(deps.storage)?;

                // first remove denoms
                for denom in to_remove {
                    if !denoms.remove(&denom) {
                        return Err(ContractError::CannotRemoveDenom(denom));
                    }
                }

                // add new denoms
                denoms.extend(to_add);

                DENOM_WHITELIST.save(deps.storage, &denoms)?;

                let event = ValenceEvent::RebalancerUpdateDenomWhitelist { denoms };

                Ok(Response::default().add_event(event.into()))
            }
            RebalancerAdminMsg::UpdateBaseDenomWhitelist { to_add, to_remove } => {
                let mut base_denoms = BASE_DENOM_WHITELIST.load(deps.storage)?;

                // first remove denoms
                for denom in to_remove {
                    let base_denom = BaseDenom::new_empty(&denom);
                    if !base_denoms.remove(&base_denom) {
                        return Err(ContractError::CannotRemoveBaseDenom(denom));
                    }
                }

                // add new denoms
                base_denoms.extend(to_add);

                BASE_DENOM_WHITELIST.save(deps.storage, &base_denoms)?;

                let event = ValenceEvent::RebalancerUpdateBaseDenomWhitelist { base_denoms };

                Ok(Response::default().add_event(event.into()))
            }
            RebalancerAdminMsg::UpdateServicesManager { addr } => {
                let addr = deps.api.addr_validate(&addr)?;

                SERVICES_MANAGER.save(deps.storage, &addr)?;

                let event = ValenceEvent::RebalancerUpdateServicesManager {
                    addr: addr.to_string(),
                };

                Ok(Response::default().add_event(event.into()))
            }
            RebalancerAdminMsg::UpdateAuctionsManager { addr } => {
                let addr = deps.api.addr_validate(&addr)?;

                AUCTIONS_MANAGER_ADDR.save(deps.storage, &addr)?;

                let event = ValenceEvent::RebalancerUpdateAuctionsManager {
                    addr: addr.to_string(),
                };

                Ok(Response::default().add_event(event.into()))
            }
            RebalancerAdminMsg::UpdateCyclePeriod { period } => {
                CYCLE_PERIOD.save(deps.storage, &period)?;

                let event = ValenceEvent::RebalancerUpdateCyclePeriod { period };

                Ok(Response::default().add_event(event.into()))
            }
            RebalancerAdminMsg::UpdateFees { fees } => {
                SERVICE_FEE_CONFIG.save(deps.storage, &fees)?;

                let event = ValenceEvent::RebalancerUpdateFees { fees };

                Ok(Response::default().add_event(event.into()))
            }
            RebalancerAdminMsg::StartAdminChange { addr, expiration } => {
                let event = ValenceEvent::RebalancerStartAdminChange {
                    admin: addr.clone(),
                };
                Ok(start_admin_change(deps, &info, &addr, expiration)?.add_event(event.into()))
            }
            RebalancerAdminMsg::CancelAdminChange {} => {
                let event = ValenceEvent::RebalancerCancelAdminChange {};
                Ok(cancel_admin_change(deps, &info)?.add_event(event.into()))
            }
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::GetConfig { addr } => {
            to_json_binary(&CONFIGS.load(deps.storage, deps.api.addr_validate(&addr)?)?)
        }
        QueryMsg::GetPausedConfig { addr } => {
            to_json_binary(&PAUSED_CONFIGS.load(deps.storage, deps.api.addr_validate(&addr)?)?)
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
        QueryMsg::GetServiceFee { account, action } => {
            let fees = SERVICE_FEE_CONFIG.load(deps.storage)?;
            let fee_amount = match action {
                QueryFeeAction::Register => fees.register_fee,
                QueryFeeAction::Resume => {
                    let Ok(paused_config) =
                        PAUSED_CONFIGS.load(deps.storage, deps.api.addr_validate(&account)?)
                    else {
                        return to_json_binary::<Option<Coin>>(&None);
                    };

                    match paused_config.reason {
                        valence_package::services::rebalancer::PauseReason::EmptyBalance => {
                            fees.resume_fee
                        }
                        valence_package::services::rebalancer::PauseReason::NotWhitelistedAccountCodeId(_) => {
                            fees.resume_fee
                        }
                        valence_package::services::rebalancer::PauseReason::AccountReason(_) => {
                            Uint128::zero()
                        }
                    }
                }
            };

            if !fee_amount.is_zero() {
                return to_json_binary(&Some(Coin {
                    denom: fees.denom,
                    amount: fee_amount,
                }));
            }

            to_json_binary::<Option<Coin>>(&None)
        }
        QueryMsg::GetAllConfigs { start_after, limit } => {
            let start_after =
                start_after.map(|addr| Bound::inclusive(deps.api.addr_validate(&addr).unwrap()));

            let configs = CONFIGS
                .range(
                    deps.storage,
                    start_after,
                    None,
                    cosmwasm_std::Order::Ascending,
                )
                .take(limit.unwrap_or(50) as usize)
                .collect::<Result<Vec<_>, StdError>>()?;

            to_json_binary(&configs)
        }
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

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, _env: Env, msg: MigrateMsg) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    match msg {
        MigrateMsg::NoStateChange {} => Ok(Response::default()),
    }
}
