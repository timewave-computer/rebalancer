use std::collections::HashSet;

#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_json_binary, Addr, BankMsg, Binary, Coin, Deps, DepsMut, Env, MessageInfo, Response,
    StdError, StdResult,
};
use cw2::set_contract_version;
use cw_storage_plus::Bound;
use valence_package::helpers::approve_admin_change;
use valence_package::msgs::core_execute::ServicesManagerExecuteMsg;
use valence_package::msgs::core_query::ServicesManagerQueryMsg;
use valence_package::services::rebalancer::RebalancerConfig;
use valence_package::states::{ACCOUNT_WHITELISTED_CODE_IDS, ADMIN};

use crate::error::ContractError;
use crate::helpers::{get_service_addr, save_service, verify_account_code_id};
use crate::msg::{InstantiateMsg, MigrateMsg};
use crate::state::{ADDR_TO_SERVICES, SERVICES_TO_ADDR};

const CONTRACT_NAME: &str = "crates.io:services-manager";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    ADMIN.save(deps.storage, &info.sender)?;

    ACCOUNT_WHITELISTED_CODE_IDS.save(
        deps.storage,
        &HashSet::from_iter(msg.whitelisted_code_ids.iter().cloned()),
    )?;

    Ok(Response::default().add_attribute("method", "instantiate"))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ServicesManagerExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ServicesManagerExecuteMsg::Admin(admin_msg) => {
            admin::handle_msg(deps, env, info, admin_msg)
        }
        ServicesManagerExecuteMsg::ApproveAdminChange {} => {
            Ok(approve_admin_change(deps, &env, &info)?)
        }
        ServicesManagerExecuteMsg::RegisterToService { service_name, data } => {
            verify_account_code_id(deps.as_ref(), &info.sender)?;

            let service_addr = get_service_addr(deps.as_ref(), service_name.to_string())?;

            let msg = service_name.get_register_msg(&info, service_addr.as_ref(), data)?;

            Ok(Response::default().add_message(msg))
        }
        ServicesManagerExecuteMsg::DeregisterFromService { service_name } => {
            let service_addr = get_service_addr(deps.as_ref(), service_name.to_string())?;

            let msg = service_name.get_deregister_msg(&info, service_addr.as_ref())?;

            Ok(Response::default().add_message(msg))
        }
        ServicesManagerExecuteMsg::UpdateService { service_name, data } => {
            let service_addr = get_service_addr(deps.as_ref(), service_name.to_string())?;

            verify_account_code_id(deps.as_ref(), &info.sender)?;

            let msg = service_name.get_update_msg(&info, service_addr.as_ref(), data)?;

            Ok(Response::default().add_message(msg))
        }
        ServicesManagerExecuteMsg::PauseService {
            service_name,
            pause_for,
            reason,
        } => {
            let service_addr = get_service_addr(deps.as_ref(), service_name.to_string())?;
            let msg =
                service_name.get_pause_msg(pause_for, &info, service_addr.as_ref(), reason)?;

            Ok(Response::default().add_message(msg))
        }
        ServicesManagerExecuteMsg::ResumeService {
            service_name,
            resume_for,
        } => {
            let service_addr = get_service_addr(deps.as_ref(), service_name.to_string())?;

            verify_account_code_id(deps.as_ref(), &resume_for)?;

            let msg = service_name.get_resume_msg(resume_for, &info, service_addr.as_ref())?;

            Ok(Response::default().add_message(msg))
        }
    }
}

mod admin {
    use valence_package::{
        event_indexing::ValenceEvent,
        helpers::{cancel_admin_change, start_admin_change, verify_admin},
        msgs::core_execute::ServicesManagerAdminMsg,
    };

    use crate::helpers::remove_service;

    use super::*;

    pub fn handle_msg(
        deps: DepsMut,
        env: Env,
        info: MessageInfo,
        msg: ServicesManagerAdminMsg,
    ) -> Result<Response, ContractError> {
        // Verify that the sender is the admin
        verify_admin(deps.as_ref(), &info)?;

        match msg {
            ServicesManagerAdminMsg::AddService { name, addr } => {
                let addr = deps.api.addr_validate(&addr)?;

                if SERVICES_TO_ADDR.has(deps.storage, name.to_string()) {
                    return Err(ContractError::ServiceAlreadyExists(name.to_string()));
                } else if ADDR_TO_SERVICES.has(deps.storage, addr.clone()) {
                    return Err(ContractError::ServiceAddressAlreadyExists(addr.to_string()));
                } else {
                    save_service(deps, name.to_string(), addr.clone())?;
                }

                let event = ValenceEvent::ServicesManagerAddService {
                    service_name: name.to_string(),
                    addr: addr.to_string(),
                };

                Ok(Response::default().add_event(event.into()))
            }
            ServicesManagerAdminMsg::UpdateService { name, addr } => {
                let addr = deps.api.addr_validate(&addr)?;

                if ADDR_TO_SERVICES.has(deps.storage, addr.clone()) {
                    return Err(ContractError::ServiceAddressAlreadyExists(addr.to_string()));
                } else if !SERVICES_TO_ADDR.has(deps.storage, name.to_string()) {
                    return Err(ContractError::ServiceDoesntExistYet(name.to_string()));
                }

                save_service(deps, name.to_string(), addr.clone())?;

                let event = ValenceEvent::ServicesManagerUpdateService {
                    service_name: name.to_string(),
                    addr: addr.to_string(),
                };

                Ok(Response::default().add_event(event.into()))
            }
            ServicesManagerAdminMsg::RemoveService { name } => {
                let addr = get_service_addr(deps.as_ref(), name.to_string())?;
                remove_service(deps, name.to_string(), addr)?;

                let event = ValenceEvent::ServicesManagerRemoveService {
                    service_name: name.to_string(),
                };

                Ok(Response::default().add_event(event.into()))
            }
            ServicesManagerAdminMsg::UpdateCodeIdWhitelist { to_add, to_remove } => {
                let mut whitelist = ACCOUNT_WHITELISTED_CODE_IDS.load(deps.storage)?;

                whitelist.extend(to_add.clone());

                for code_id in &to_remove {
                    if !whitelist.remove(code_id) {
                        return Err(ContractError::CodeIdNotInWhitelist(*code_id));
                    }
                }

                ACCOUNT_WHITELISTED_CODE_IDS.save(deps.storage, &whitelist)?;

                let event =
                    ValenceEvent::ServicesManagerUpdateCodeIdWhitelist { to_add, to_remove };
                Ok(Response::default().add_event(event.into()))
            }
            ServicesManagerAdminMsg::StartAdminChange { addr, expiration } => {
                let event = ValenceEvent::ServicesManagerStartAdminChange {
                    admin: addr.clone(),
                };
                Ok(start_admin_change(deps, &info, &addr, expiration)?.add_event(event.into()))
            }
            ServicesManagerAdminMsg::CancelAdminChange {} => {
                let event = ValenceEvent::ServicesManagerCancelAdminChange {};
                Ok(cancel_admin_change(deps, &info)?.add_event(event.into()))
            }
            ServicesManagerAdminMsg::Withdraw { denom } => {
                let amount = deps.querier.query_balance(env.contract.address, denom)?;

                let msg = BankMsg::Send {
                    to_address: info.sender.to_string(), // sender must be admin
                    amount: vec![amount.clone()],
                };

                let event = ValenceEvent::ServicesManagerWithdraw { amount };

                Ok(Response::default().add_event(event.into()).add_message(msg))
            }
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: ServicesManagerQueryMsg) -> StdResult<Binary> {
    match msg {
        ServicesManagerQueryMsg::IsService { addr } => {
            let is_service = ADDR_TO_SERVICES.has(deps.storage, deps.api.addr_validate(&addr)?);
            to_json_binary(&is_service)
        }
        ServicesManagerQueryMsg::IsAccountCodeId { code_id } => {
            let code_ids = ACCOUNT_WHITELISTED_CODE_IDS.load(deps.storage)?;
            to_json_binary(&code_ids.contains(&code_id))
        }
        ServicesManagerQueryMsg::GetServiceAddr { service } => {
            let addr = get_service_addr(deps, service.to_string())
                .map_err(|e| StdError::GenericErr { msg: e.to_string() })?;
            to_json_binary(&addr)
        }
        ServicesManagerQueryMsg::GetAdmin => to_json_binary(&ADMIN.load(deps.storage)?),
        ServicesManagerQueryMsg::GetAllServices { start_from, limit } => {
            let start_from = start_from.map(Bound::exclusive);
            let limit = limit.unwrap_or(50) as usize;

            let services = SERVICES_TO_ADDR
                .range(
                    deps.storage,
                    start_from,
                    None,
                    cosmwasm_std::Order::Ascending,
                )
                .take(limit)
                .collect::<StdResult<Vec<(String, Addr)>>>()?;

            to_json_binary(&services)
        }
        ServicesManagerQueryMsg::GetServiceFee {
            account,
            service,
            action,
        } => {
            let service_addr = SERVICES_TO_ADDR.load(deps.storage, service.to_string())?;

            let fee = deps.querier.query_wasm_smart::<Option<Coin>>(
                service_addr,
                &rebalancer::msg::QueryMsg::GetServiceFee { account, action },
            )?;

            to_json_binary(&fee)
        }
        ServicesManagerQueryMsg::GetRebalancerConfig { account } => {
            let service_addr = SERVICES_TO_ADDR.load(deps.storage, "rebalancer".to_string())?;
            let config = deps.querier.query_wasm_smart::<RebalancerConfig>(
                service_addr,
                &rebalancer::msg::QueryMsg::GetConfig { addr: account },
            )?;

            to_json_binary(&config)
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, _env: Env, msg: MigrateMsg) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    match msg {
        MigrateMsg::NoStateChange {} => Ok(Response::default()),
    }
}
