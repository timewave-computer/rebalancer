use auction_package::helpers::{approve_admin_change, GetPriceResponse};
use auction_package::msgs::AuctionsManagerQueryMsg;
use auction_package::states::{
    MinAmount, ADMIN, MIN_AUCTION_AMOUNT, MIN_AUCTION_AMOUNT_V0, ORACLE_ADDR, PAIRS,
};
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_json_binary, Binary, Deps, DepsMut, Env, MessageInfo, Order, Reply, Response, StdResult,
    WasmMsg,
};
use cw2::set_contract_version;
use cw_storage_plus::Bound;
use cw_utils::{nonpayable, parse_reply_instantiate_data};
use valence_package::event_indexing::ValenceEvent;

use crate::error::ContractError;
use crate::msg::{ExecuteMsg, InstantiateMsg, MigrateMsg};
use crate::state::{AUCTION_CODE_ID, SERVER_ADDR};

const CONTRACT_NAME: &str = "crates.io:auctions-manager";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

const INSTANTIATE_AUCTION_REPLY_ID: u64 = 1;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    ADMIN.save(deps.storage, &info.sender)?;
    SERVER_ADDR.save(deps.storage, &deps.api.addr_validate(&msg.server_addr)?)?;
    AUCTION_CODE_ID.save(deps.storage, &msg.auction_code_id)?;

    for min_amount in msg.min_auction_amount {
        MIN_AUCTION_AMOUNT.save(deps.storage, min_amount.0, &min_amount.1)?;
    }

    Ok(Response::default().add_attribute("method", "instantiate"))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::AuctionFunds { pair } => {
            pair.verify()?;
            let pair_addr = PAIRS.load(deps.storage, pair)?;

            let msg = WasmMsg::Execute {
                contract_addr: pair_addr.to_string(),
                msg: to_json_binary(&auction::msg::ExecuteMsg::AuctionFundsManager {
                    sender: info.sender,
                })?,
                funds: info.funds,
            };

            Ok(Response::default().add_message(msg))
        }
        ExecuteMsg::WithdrawFunds { pair } => {
            nonpayable(&info)?;
            pair.verify()?;
            let pair_addr = PAIRS.load(deps.storage, pair)?;

            let msg = WasmMsg::Execute {
                contract_addr: pair_addr.to_string(),
                msg: to_json_binary(&auction::msg::ExecuteMsg::WithdrawFundsManager {
                    sender: info.sender,
                })?,
                funds: vec![],
            };

            Ok(Response::default().add_message(msg))
        }
        ExecuteMsg::FinishAuction { pair, limit } => {
            nonpayable(&info)?;
            pair.verify()?;
            let pair_addr = PAIRS.load(deps.storage, pair)?;

            let msg = WasmMsg::Execute {
                contract_addr: pair_addr.to_string(),
                msg: to_json_binary(&auction::msg::ExecuteMsg::FinishAuction { limit })?,
                funds: vec![],
            };

            Ok(Response::default().add_message(msg))
        }
        ExecuteMsg::Admin(admin_msg) => admin::handle_msg(deps, env, info, *admin_msg),
        ExecuteMsg::Server(server_msg) => server::handle_msg(deps, env, info, server_msg),
        ExecuteMsg::ApproveAdminChange {} => {
            let event = ValenceEvent::AuctionManagerApproveAdminChange {};
            Ok(approve_admin_change(deps, &env, &info)?.add_event(event.into()))
        }
    }
}

mod server {
    use cosmwasm_std::{ensure, to_json_binary, WasmMsg};

    use crate::msg::ServerMsgs;

    use super::*;

    pub fn handle_msg(
        deps: DepsMut,
        _env: Env,
        info: MessageInfo,
        msg: ServerMsgs,
    ) -> Result<Response, ContractError> {
        // Verify that the sender is the server
        let server_addr = SERVER_ADDR.load(deps.storage)?;
        ensure!(info.sender == server_addr, ContractError::NotServer);

        match msg {
            ServerMsgs::OpenAuction { pair, params } => {
                let pair_addr = PAIRS.load(deps.storage, pair)?;
                let msg = WasmMsg::Execute {
                    contract_addr: pair_addr.to_string(),
                    msg: to_json_binary(&auction::msg::ExecuteMsg::Admin(Box::new(
                        auction::msg::AdminMsgs::StartAuction(params),
                    )))?,
                    funds: vec![],
                };

                Ok(Response::default().add_message(msg))
            }
        }
    }
}

mod admin {
    use auction_package::helpers::{cancel_admin_change, start_admin_change, verify_admin};
    use cosmwasm_std::{to_json_binary, SubMsg, WasmMsg};
    use valence_package::event_indexing::{ValenceEvent, ValenceGenericEvent};

    use crate::msg::AdminMsgs;

    use super::*;

    pub fn handle_msg(
        deps: DepsMut,
        env: Env,
        info: MessageInfo,
        msg: AdminMsgs,
    ) -> Result<Response, ContractError> {
        // Verify that the sender is the admin
        verify_admin(deps.as_ref(), &info)?;

        match msg {
            AdminMsgs::NewAuction {
                msg,
                label,
                min_amount,
            } => {
                msg.pair.verify()?;

                // Make sure we either set min_amount, or have it in storage
                match min_amount {
                    Some(min_amount) => {
                        MIN_AUCTION_AMOUNT.save(deps.storage, msg.pair.0.clone(), &min_amount)?;
                        Ok::<_, ContractError>(())
                    }
                    None => {
                        MIN_AUCTION_AMOUNT
                            .load(deps.storage, msg.pair.0.clone())
                            .map_err(|_| {
                                ContractError::MustSetMinAuctionAmount(msg.pair.0.clone())
                            })?;
                        Ok(())
                    }
                }?;

                let init_msg = SubMsg::reply_on_success(
                    WasmMsg::Instantiate {
                        admin: Some(env.contract.address.to_string()),
                        code_id: AUCTION_CODE_ID.load(deps.storage)?,
                        msg: to_json_binary(&msg)?,
                        funds: vec![],
                        label,
                    },
                    INSTANTIATE_AUCTION_REPLY_ID,
                );

                Ok(Response::default().add_submessage(init_msg))
            }
            AdminMsgs::PauseAuction { pair } => {
                let pair_addr = PAIRS.load(deps.storage, pair)?;
                let msg = WasmMsg::Execute {
                    contract_addr: pair_addr.to_string(),
                    msg: to_json_binary(&auction::msg::ExecuteMsg::Admin(Box::new(
                        auction::msg::AdminMsgs::PauseAuction,
                    )))?,
                    funds: vec![],
                };

                Ok(Response::default().add_message(msg))
            }
            AdminMsgs::ResumeAuction { pair } => {
                let pair_addr = PAIRS.load(deps.storage, pair)?;
                let msg = WasmMsg::Execute {
                    contract_addr: pair_addr.to_string(),
                    msg: to_json_binary(&auction::msg::ExecuteMsg::Admin(Box::new(
                        auction::msg::AdminMsgs::ResumeAuction,
                    )))?,
                    funds: vec![],
                };

                Ok(Response::default().add_message(msg))
            }
            AdminMsgs::UpdateAuctionId { code_id } => {
                AUCTION_CODE_ID.save(deps.storage, &code_id)?;

                let event = ValenceEvent::AuctionManagerUpdateAuctionCodeId { code_id };

                Ok(Response::default().add_event(event.into()))
            }
            AdminMsgs::UpdateOracle { oracle_addr } => {
                ORACLE_ADDR.save(deps.storage, &deps.api.addr_validate(&oracle_addr)?)?;

                let event = ValenceEvent::AuctionManagerUpdateOracle { oracle_addr };

                Ok(Response::default().add_event(event.into()))
            }
            AdminMsgs::UpdateStrategy { pair, strategy } => {
                let pair_addr = PAIRS.load(deps.storage, pair)?;
                let msg = WasmMsg::Execute {
                    contract_addr: pair_addr.to_string(),
                    msg: to_json_binary(&auction::msg::ExecuteMsg::Admin(Box::new(
                        auction::msg::AdminMsgs::UpdateStrategy { strategy },
                    )))?,
                    funds: vec![],
                };

                Ok(Response::default().add_message(msg))
            }
            AdminMsgs::UpdateChainHaltConfig { pair, halt_config } => {
                let pair_addr = PAIRS.load(deps.storage, pair)?;
                let msg = WasmMsg::Execute {
                    contract_addr: pair_addr.to_string(),
                    msg: to_json_binary(&auction::msg::ExecuteMsg::Admin(Box::new(
                        auction::msg::AdminMsgs::UpdateChainHaltConfig(halt_config),
                    )))?,
                    funds: vec![],
                };

                Ok(Response::default().add_message(msg))
            }
            AdminMsgs::UpdatePriceFreshnessStrategy { pair, strategy } => {
                let pair_addr = PAIRS.load(deps.storage, pair)?;
                let msg = WasmMsg::Execute {
                    contract_addr: pair_addr.to_string(),
                    msg: to_json_binary(&auction::msg::ExecuteMsg::Admin(Box::new(
                        auction::msg::AdminMsgs::UpdatePriceFreshnessStrategy(strategy),
                    )))?,
                    funds: vec![],
                };

                Ok(Response::default().add_message(msg))
            }
            AdminMsgs::MigrateAuction { pair, code_id, msg } => {
                let pair_addr = PAIRS.load(deps.storage, pair.clone())?;

                let migrate_msg = WasmMsg::Migrate {
                    contract_addr: pair_addr.to_string(),
                    msg: to_json_binary(&msg)?,
                    new_code_id: code_id,
                };

                let event =
                    ValenceGenericEvent::<auction::msg::MigrateMsg>::AuctionManagerMigrateAuction {
                        pair,
                        code_id,
                        msg,
                    };

                Ok(Response::default()
                    .add_event(event.into())
                    .add_message(migrate_msg))
            }
            AdminMsgs::UpdateMinAmount { denom, min_amount } => {
                MIN_AUCTION_AMOUNT.save(deps.storage, denom.clone(), &min_amount)?;

                let event = ValenceEvent::AuctionManagerUpdateMinAmount { denom, min_amount };

                Ok(Response::default().add_event(event.into()))
            }
            AdminMsgs::ChangeServerAddr { addr } => {
                SERVER_ADDR.save(deps.storage, &deps.api.addr_validate(&addr)?)?;

                let event = ValenceEvent::AuctionManagerChangeServerAddr { addr };

                Ok(Response::default().add_event(event.into()))
            }
            AdminMsgs::StartAdminChange { addr, expiration } => {
                let event = ValenceEvent::AuctionManagerStartAdminChange {
                    admin: addr.clone(),
                };
                Ok(start_admin_change(deps, &info, &addr, expiration)?.add_event(event.into()))
            }
            AdminMsgs::CancelAdminChange {} => {
                let event = ValenceEvent::AuctionManagerCancelAdminChange {};
                Ok(cancel_admin_change(deps, &info)?.add_event(event.into()))
            }
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: AuctionsManagerQueryMsg) -> StdResult<Binary> {
    match msg {
        AuctionsManagerQueryMsg::GetPairs { start_after, limit } => {
            let start_after = start_after.map(Bound::exclusive);
            let pairs = PAIRS
                .range(deps.storage, start_after, None, Order::Ascending)
                .take(limit.unwrap_or(50) as usize)
                .collect::<StdResult<Vec<_>>>()?;

            to_json_binary(&pairs)
        }
        AuctionsManagerQueryMsg::GetPairAddr { pair } => {
            to_json_binary(&PAIRS.load(deps.storage, pair)?)
        }
        AuctionsManagerQueryMsg::GetPrice { pair } => {
            let oracle_addr = ORACLE_ADDR
                .load(deps.storage)
                .map_err(|_| ContractError::OracleAddrMissing)?;

            to_json_binary(&deps.querier.query_wasm_smart::<GetPriceResponse>(
                oracle_addr,
                &price_oracle::msg::QueryMsg::GetPrice { pair },
            )?)
        }
        AuctionsManagerQueryMsg::GetOracleAddr => to_json_binary(&ORACLE_ADDR.load(deps.storage)?),
        AuctionsManagerQueryMsg::GetConfig { pair } => {
            let pair_addr = PAIRS.load(deps.storage, pair)?;
            deps.querier
                .query_wasm_smart(pair_addr, &auction::msg::QueryMsg::GetConfig)
        }
        AuctionsManagerQueryMsg::GetMinLimit { denom } => {
            to_json_binary(&MIN_AUCTION_AMOUNT.load(deps.storage, denom)?)
        }
        AuctionsManagerQueryMsg::GetAdmin => to_json_binary(&ADMIN.load(deps.storage)?),
        AuctionsManagerQueryMsg::GetServerAddr => to_json_binary(&SERVER_ADDR.load(deps.storage)?),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, _env: Env, msg: Reply) -> Result<Response, ContractError> {
    match msg.id {
        INSTANTIATE_AUCTION_REPLY_ID => {
            let auction_addr = deps
                .api
                .addr_validate(&parse_reply_instantiate_data(msg)?.contract_address)?;
            let auction_config: auction_package::helpers::AuctionConfig = deps
                .querier
                .query_wasm_smart(auction_addr.clone(), &auction::msg::QueryMsg::GetConfig)?;

            PAIRS.save(deps.storage, auction_config.pair, &auction_addr)?;

            Ok(Response::default())
        }
        _ => Err(ContractError::UnknownReplyId(msg.id)),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, _env: Env, msg: MigrateMsg) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    match msg {
        MigrateMsg::NoStateChange {} => Ok(Response::default()),
        MigrateMsg::ToV1 {} => {
            let mins = MIN_AUCTION_AMOUNT_V0
                .range(deps.storage, None, None, cosmwasm_std::Order::Ascending)
                .collect::<StdResult<Vec<_>>>()?;

            mins.iter().for_each(|(denom, amount)| {
                MIN_AUCTION_AMOUNT
                    .save(
                        deps.storage,
                        denom.to_string(),
                        &MinAmount {
                            send: *amount,
                            start_auction: *amount,
                        },
                    )
                    .unwrap();
            });

            Ok(Response::default())
        }
    }
}
