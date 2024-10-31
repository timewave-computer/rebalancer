use std::{borrow::BorrowMut, collections::HashMap, str::FromStr};

use auction_package::{
    helpers::GetPriceResponse,
    states::{MinAmount, MIN_AUCTION_AMOUNT, PAIRS},
    Pair,
};
use cosmwasm_std::{
    coins, to_json_binary, Addr, CosmosMsg, Decimal, Deps, DepsMut, Empty, Env, Event, Order,
    Response, SignedDecimal, StdError, SubMsg, Uint128, WasmMsg,
};
use cw_storage_plus::Bound;
use valence_package::{
    event_indexing::ValenceEvent,
    helpers::start_of_cycle,
    services::rebalancer::{
        ParsedPID, PauseData, RebalanceTrade, RebalancerConfig, SystemRebalanceStatus,
        TargetOverrideStrategy,
    },
    states::{ACCOUNT_WHITELISTED_CODE_IDS, SERVICES_MANAGER},
    CLOSEST_TO_ONE_POSSIBLE,
};

use crate::{
    contract::{DEFAULT_SYSTEM_LIMIT, REPLY_DEFAULT_REBALANCE},
    error::ContractError,
    helpers::{RebalanceResponse, TargetHelper, TradesTuple},
    state::{
        AUCTIONS_MANAGER_ADDR, BASE_DENOM_WHITELIST, CONFIGS, CYCLE_PERIOD, DENOM_WHITELIST,
        PAUSED_CONFIGS, SYSTEM_REBALANCE_STATUS,
    },
};

const MAX_PID_DT_VALUE: u128 = 10;

/// Main function for rebalancing using the system
pub fn execute_system_rebalance(
    mut deps: DepsMut,
    env: &Env,
    limit: Option<u64>,
) -> Result<Response, ContractError> {
    let cycle_period = CYCLE_PERIOD.load(deps.storage)?;
    let limit = limit.unwrap_or(DEFAULT_SYSTEM_LIMIT) as usize;

    if limit == 0 {
        return Err(ContractError::LimitIsZero);
    }
    // start_from tells us if we should start form a specific addr or from the begining
    // cycle_start tells us when the cycle started to calculate for processing and finished status
    let (start_from, cycle_start, prices) = match SYSTEM_REBALANCE_STATUS.load(deps.storage)? {
        SystemRebalanceStatus::NotStarted { cycle_start } => {
            if env.block.time < cycle_start {
                Err(ContractError::CycleNotStartedYet(cycle_start.seconds()))
            } else {
                Ok((None, start_of_cycle(env.block.time, cycle_period), None))
            }
        }
        SystemRebalanceStatus::Processing {
            cycle_started,
            start_from,
            prices,
        } => {
            if env.block.time >= cycle_started.plus_seconds(cycle_period) {
                Ok((None, start_of_cycle(env.block.time, cycle_period), None))
            } else {
                Ok((Some(start_from), cycle_started, Some(prices)))
            }
        }
        SystemRebalanceStatus::Finished { next_cycle } => {
            if env.block.time < next_cycle {
                Err(ContractError::CycleNotStartedYet(next_cycle.seconds()))
            } else {
                Ok((None, next_cycle, None))
            }
        }
    }?;

    let auction_manager = AUCTIONS_MANAGER_ADDR.load(deps.storage)?;

    let prices = match prices {
        Some(prices) => Ok(prices),
        None => get_prices(deps.borrow_mut(), &auction_manager),
    }?;

    // `start_from` is the last address we looped over in the previous message
    // if exists we do have an address we should continue from
    // if its None, then we start the loop from the begining.
    let mut last_addr = start_from.clone();
    let start_from = start_from.map(Bound::exclusive);

    let mut configs = CONFIGS
        .range(deps.storage, start_from, None, Order::Ascending)
        .take(limit + 1)
        .collect::<Vec<Result<(Addr, RebalancerConfig), StdError>>>();

    // Get the length of configs to check if we finished looping over all accounts
    let configs_len = configs.len();

    // If we took more then our limit (limit +1) than we have more to loop
    // remove last element and loop only over the limit amount
    if configs_len > limit {
        configs.remove(configs_len - 1)?;
    }

    // get base denoms as hashMap
    let base_denoms_min_values = BASE_DENOM_WHITELIST
        .load(deps.storage)?
        .iter()
        .map(|bd| (bd.denom.clone(), bd.min_balance_limit))
        .collect::<HashMap<String, Uint128>>();

    let mut min_amount_limits: Vec<(String, Uint128)> = vec![];
    let mut msgs: Vec<SubMsg> = vec![];
    let mut account_events: Vec<Event> = vec![];
    let services_manager_addr = SERVICES_MANAGER.load(deps.storage)?;
    let whitelist = ACCOUNT_WHITELISTED_CODE_IDS.query(&deps.querier, services_manager_addr)?;

    for res in configs {
        let Ok((account, config)) = res else {
            continue;
        };

        last_addr = Some(account.clone());

        // Before rebalancing, verify the account is using a whitelisted code id
        let account_code_id = deps
            .querier
            .query_wasm_contract_info(account.clone())?
            .code_id;

        if !whitelist.contains(&account_code_id) {
            // Save to the paused config
            PAUSED_CONFIGS.save(
                deps.storage,
                account.clone(),
                &PauseData::new_not_whitelisted_account_code_id(env, account_code_id, &config),
            )?;
            // remove from active configs
            CONFIGS.remove(deps.storage, account.clone());
            continue;
        }

        // Do rebalance for the account, and construct the msg
        let rebalance_res = do_rebalance(
            deps.as_ref(),
            env,
            &account,
            &auction_manager,
            config,
            &mut min_amount_limits,
            &base_denoms_min_values,
            &prices,
            cycle_period,
        );
        let Ok(RebalanceResponse {
            config,
            msg,
            event,
            should_pause,
        }) = rebalance_res
        else {
            account_events.push(
                Event::new("rebalancer-error")
                    .add_attribute("error", rebalance_res.unwrap_err().to_string()),
            );
            continue;
        };

        // check if we should pause the account or not.
        if should_pause {
            // Save to the paused config
            PAUSED_CONFIGS.save(
                deps.storage,
                account.clone(),
                &PauseData::new_empty_balance(env, &config),
            )?;
            // remove from active configs
            CONFIGS.remove(deps.storage, account);
        } else {
            // Rebalacing modify the config to include the latest data available to us
            // as well as some rebalancing data we need for the next rebalance cycle
            CONFIGS.save(deps.branch().storage, account, &config)?;
        }

        // Add event to all events
        account_events.push(event.into());

        if let Some(msg) = msg {
            msgs.push(msg);
        }
    }

    // We checked if we finished looping over all accounts or not
    // and set the status based on that
    let status = if configs_len <= limit {
        SystemRebalanceStatus::Finished {
            next_cycle: cycle_start.plus_seconds(cycle_period),
        }
    } else {
        SystemRebalanceStatus::Processing {
            cycle_started: cycle_start,
            start_from: last_addr.unwrap(),
            prices,
        }
    };

    SYSTEM_REBALANCE_STATUS.save(deps.storage, &status)?;

    let event = ValenceEvent::RebalancerCycle {
        limit: limit as u64,
        cycled_over: configs_len as u64,
    };

    Ok(Response::default()
        .add_event(event.into())
        .add_events(account_events)
        .add_submessages(msgs))
}

/// Make sure the balance of the account is not zero and is above our minimum value
fn verify_account_balance(total_value: Uint128, min_value: Uint128) -> Result<(), ContractError> {
    if total_value.is_zero() {
        return Err(ContractError::AccountBalanceIsZero);
    }

    if total_value < min_value {
        return Err(ContractError::InvalidAccountMinValue(
            total_value.to_string(),
            min_value.to_string(),
        ));
    }

    Ok(())
}

/// Do a rebalance with PID calculation for a single account
#[allow(clippy::too_many_arguments)]
pub fn do_rebalance(
    deps: Deps,
    env: &Env,
    account: &Addr,
    auction_manager: &Addr,
    mut config: RebalancerConfig,
    min_amount_limits: &mut Vec<(String, Uint128)>,
    min_values: &HashMap<String, Uint128>,
    prices: &[(Pair, Decimal)],
    cycle_period: u64,
) -> Result<RebalanceResponse<Empty>, ContractError> {
    // get a vec of inputs for our calculations
    let (total_value, mut target_helpers) = get_inputs(deps, account, &config, prices)?;

    // Get required minim
    let min_value = *min_values
        .get(config.base_denom.as_str())
        .unwrap_or(&Uint128::zero());

    if verify_account_balance(total_value.to_uint_floor(), min_value).is_err() {
        let event = ValenceEvent::RebalancerAccountRebalancePause {
            account: account.to_string(),
            total_value,
        };

        // We pause the account if the account balance doesn't meet the minimum requirements
        return Ok(RebalanceResponse::new(config, None, event, true));
    };

    // Verify the targets, if we have a min_balance we need to do some extra steps
    // to make sure min_balance is accounted for in our calculations
    if config.has_min_balance {
        target_helpers = verify_targets(&config, total_value, target_helpers)?;
    }

    // Calc the time delta for our PID calculation
    let dt = if config.last_rebalance.seconds() == 0 {
        Decimal::one()
    } else {
        let diff = Decimal::from_atomics(
            env.block.time.seconds() - config.last_rebalance.seconds(),
            0,
        )?;
        (diff.checked_div(Decimal::from_atomics(cycle_period, 0)?))?
            .min(Decimal::from_atomics(MAX_PID_DT_VALUE, 0)?)
    };

    let (mut to_sell, to_buy) = do_pid(
        total_value,
        &mut target_helpers,
        config.pid.clone(),
        dt.try_into()?,
    )?;

    // Update targets in config only the last data we need for the next rebalance calculation
    for target in config.targets.iter_mut() {
        if let Some(target_helper) = target_helpers
            .iter()
            .find(|th| th.target.denom == target.denom)
        {
            target.update_last(&target_helper.target);
        }
    }

    // get minimum amount we can send to each auction
    set_auction_min_amounts(deps, auction_manager, &mut to_sell, min_amount_limits)?;

    // Generate the trades msgs, how much funds to send to what auction.
    let (msgs, trades) =
        generate_trades_msgs(deps, to_sell, to_buy, auction_manager, &config, total_value);

    // Construct the msg we need to execute on the account
    // Notice the atomic false, it means each trade msg (sending funds to specific pair auction)
    // is independent of other trade msg
    // This means 1 trade might fail while another pass, which means rebalance strategy was not executed 100% this cycle
    // but this will be corrected on the next rebalance cycle.
    let reb_msg = if !msgs.is_empty() {
        match config.account_type {
            valence_package::services::rebalancer::RebalancerAccountType::Regular => {
                Some(to_json_binary(
                    &valence_package::msgs::core_execute::AccountBaseExecuteMsg::SendFundsByService {
                        msgs,
                        atomic: false,
                    },
                )?)
            },
            valence_package::services::rebalancer::RebalancerAccountType::Workflow => {
                let msgs = msgs.into_iter().map(|msg| {
                    SubMsg::reply_on_error(msg, 0)
                }).collect();

                Some(to_json_binary(
                    &valence_package::services::rebalancer::MockWorkflowExecuteMsg::ExecuteSubmsgs {
                        msgs,
                        payload: None,
                    },
                )?)
            },
        }
    } else {
        None
    };

    let msg = reb_msg.map(|reb_msg| {
        SubMsg::reply_on_error(
            WasmMsg::Execute {
                contract_addr: account.to_string(),
                msg: reb_msg,
                funds: vec![],
            },
            REPLY_DEFAULT_REBALANCE,
        )
    });

    // We edit config to save data for the next rebalance calculation
    config.last_rebalance = env.block.time;

    let event = ValenceEvent::RebalancerAccountRebalance {
        account: account.to_string(),
        total_value,
        trades,
    };

    Ok(RebalanceResponse::new(config, msg, event, false))
}

/// Set the min amount an auction is willing to accept for a specific token
/// If we have it in our min_amount_limit list, we take it from there
/// if not, we query the auction to get the min amount
pub(crate) fn set_auction_min_amounts(
    deps: Deps,
    auction_manager: &Addr,
    to_sell: &mut Vec<TargetHelper>,
    min_amount_limits: &mut Vec<(String, Uint128)>,
) -> Result<(), ContractError> {
    for sell_token in to_sell {
        match min_amount_limits
            .iter()
            .find(|min_amount| min_amount.0 == sell_token.target.denom)
        {
            Some(min_amount) => {
                sell_token.auction_min_send_value =
                    Decimal::from_atomics(min_amount.1, 0)?.checked_div(sell_token.price)?;
            }
            None => {
                match MIN_AUCTION_AMOUNT.query(
                    &deps.querier,
                    auction_manager.clone(),
                    sell_token.target.denom.clone(),
                )? {
                    Some(MinAmount {
                        send: min_send_amount,
                        ..
                    }) => {
                        sell_token.auction_min_send_value =
                            Decimal::from_atomics(min_send_amount, 0)?
                                .checked_div(sell_token.price)?;
                        min_amount_limits.push((sell_token.target.denom.clone(), min_send_amount));
                        Ok(())
                    }
                    None => Err(ContractError::NoMinAuctionAmountFound),
                }?;
            }
        }
    }

    Ok(())
}

/// Get the prices for all whitelisted tokens
pub fn get_prices(
    deps: &mut DepsMut,
    auctions_manager_addr: &Addr,
) -> Result<Vec<(Pair, Decimal)>, ContractError> {
    let base_denoms = BASE_DENOM_WHITELIST.load(deps.storage)?;
    let denoms = DENOM_WHITELIST.load(deps.storage)?;
    let mut prices: Vec<(Pair, Decimal)> = vec![];

    for base_denom in base_denoms {
        for denom in &denoms {
            if &base_denom.denom == denom {
                continue;
            }

            let pair = Pair::from((base_denom.denom.clone(), denom.clone()));

            let price = deps
                .querier
                .query_wasm_smart::<GetPriceResponse>(
                    auctions_manager_addr,
                    &auction_package::msgs::AuctionsManagerQueryMsg::GetPrice {
                        pair: pair.clone(),
                    },
                )?
                .price;

            if price.is_zero() {
                return Err(ContractError::PairPriceIsZero(pair.0, pair.1));
            }

            prices.push((pair, price));
        }
    }

    Ok(prices)
}

/// Get the inputs for our calculations from the targets (current balance)
/// Returns the total value of the account, and a vec of targets with their info
fn get_inputs(
    deps: Deps,
    account: &Addr,
    config: &RebalancerConfig,
    prices: &[(Pair, Decimal)],
) -> Result<(Decimal, Vec<TargetHelper>), ContractError> {
    // get inputs per target (balance amount / price),
    // and current total input of the account (vec![denom / price].sum())
    config.targets.iter().try_fold(
        (Decimal::zero(), vec![]),
        |(mut total_value, mut targets_helpers), target| {
            // Get the price of the denom, compared to the base denom,
            // if the target denom is the base denom, we set the price to 1
            let price = if target.denom == config.base_denom {
                Decimal::one()
            } else {
                prices
                    .iter()
                    .find(|(pair, _)| pair.0 == config.base_denom && pair.1 == target.denom)
                    // we can safely unwrap here as we are 100% sure we have all prices for the whitelisted targets
                    .unwrap()
                    .1
            };

            // Get current balance of the target, and calculate the value
            // safe if balance is 0, 0 / price = 0
            let current_balance = deps.querier.query_balance(account, target.denom.clone())?;
            let balance_value =
                Decimal::from_atomics(current_balance.amount, 0)?.checked_div(price)?;

            total_value += balance_value;
            targets_helpers.push(TargetHelper {
                target: target.clone(),
                balance_amount: current_balance.amount,
                price,
                balance_value,
                value_to_trade: Decimal::zero(),
                auction_min_send_value: Decimal::zero(),
            });

            Ok((total_value, targets_helpers))
        },
    )
}

/// Do the PID calculation for the targets
fn do_pid(
    total_value: Decimal,
    targets: &mut [TargetHelper],
    pid: ParsedPID,
    dt: SignedDecimal,
) -> Result<TradesTuple, ContractError> {
    let mut to_sell: Vec<TargetHelper> = vec![];
    let mut to_buy: Vec<TargetHelper> = vec![];

    for target in targets.iter_mut() {
        let signed_input: SignedDecimal = target.balance_value.try_into()?;

        // Reset to trade value
        target.value_to_trade = Decimal::zero();

        let target_value: SignedDecimal = (total_value * target.target.percentage).try_into()?;

        let error = target_value - signed_input;

        let p = error * pid.p;
        let i = target.target.last_i + (error * pid.i * dt);
        let mut d = match target.target.last_input {
            Some(last_input) => signed_input - last_input,
            None => SignedDecimal::zero(),
        };

        d = d * pid.d / dt;

        let output = p + i - d;

        target.value_to_trade = output.abs_diff(SignedDecimal::zero());

        target.target.last_input = Some(target.balance_value.try_into()?);
        target.target.last_i = i;

        if output.is_zero() {
            continue;
        }

        match !output.is_negative() {
            // output is negative, we need to sell
            false => to_sell.push(target.clone()),
            // output is positive, we need to buy
            true => to_buy.push(target.clone()),
        }
    }

    Ok((to_sell, to_buy))
}

/// Verify the targets are correct based on min_balance
pub fn verify_targets(
    config: &RebalancerConfig,
    total_value: Decimal,
    targets: Vec<TargetHelper>,
) -> Result<Vec<TargetHelper>, ContractError> {
    let target = targets
        .iter()
        .find(|t| t.target.min_balance.is_some())
        .ok_or(ContractError::NoMinBalanceTargetFound)?
        .clone();

    // Safe to unwrap here, because we only enter the function is there is a min_balance target
    // and we error out if we don't find the target above.
    let min_balance = Decimal::from_atomics(target.target.min_balance.unwrap(), 0)?;
    let min_balance_target = min_balance / target.price;
    let real_target = total_value * target.target.percentage;

    // if the target is below the minimum balance target
    let new_targets = if real_target < min_balance_target {
        // the target is below min_balance, so we set the min_balance as the new target

        // Verify that min_balance is not higher then our total value, if it is, then we sell everything to fulfill it.
        let (new_target_perc, mut leftover_perc) = if min_balance_target >= total_value {
            (Decimal::one(), Decimal::zero())
        } else {
            let perc = min_balance_target.checked_div(total_value)?;
            (perc, Decimal::one() - perc)
        };

        let old_leftover_perc = Decimal::one() - target.target.percentage;
        let mut new_total_perc = new_target_perc;

        let updated_targets = targets
            .into_iter()
            .map(|mut t| -> Result<TargetHelper, ContractError> {
                // If our target is the min_balance target, we update perc, and return t.
                if t.target.denom == target.target.denom {
                    t.target.percentage = new_target_perc;
                    return Ok(t);
                };

                // If leftover perc is 0, we set the perc as zero for this target
                if leftover_perc.is_zero() {
                    t.target.percentage = Decimal::zero();
                    return Ok(t);
                }

                // Calc new perc based on chosen strategy and new min_balance perc
                match config.target_override_strategy {
                    TargetOverrideStrategy::Proportional => {
                        let old_perc = t.target.percentage.checked_div(old_leftover_perc)?;
                        t.target.percentage = old_perc * leftover_perc;
                    }
                    TargetOverrideStrategy::Priority => {
                        if leftover_perc >= t.target.percentage {
                            leftover_perc -= t.target.percentage;
                        } else {
                            t.target.percentage = leftover_perc;
                            leftover_perc = Decimal::zero();
                        }
                    }
                }

                new_total_perc += t.target.percentage;
                Ok(t)
            })
            .collect::<Result<Vec<_>, ContractError>>()?;

        // If the new percentage is smaller then 0.9999 or higher then 1, we have something wrong in calculation
        if new_total_perc > Decimal::one()
            || new_total_perc < Decimal::from_str(CLOSEST_TO_ONE_POSSIBLE)?
        {
            return Err(ContractError::InvalidTargetPercentage(
                new_total_perc.to_string(),
            ));
        }

        updated_targets
    } else {
        // Everything is good, we do nothing
        targets
    };

    Ok(new_targets)
}

/// Construct the messages the account need to exeucte (send funds to auctions)
fn construct_msg(
    deps: Deps,
    auction_manager: Addr,
    trade: RebalanceTrade,
) -> Result<CosmosMsg, ContractError> {
    let Some(pair_addr) = PAIRS.query(&deps.querier, auction_manager, trade.pair.clone())? else {
        return Err(ContractError::PairDoesntExists(trade.pair.0, trade.pair.1));
    };

    let msg = WasmMsg::Execute {
        contract_addr: pair_addr.to_string(),
        msg: to_json_binary(&auction::msg::ExecuteMsg::AuctionFunds {})?,
        funds: coins(trade.amount.u128(), trade.pair.0),
    };

    Ok(msg.into())
}

/// Generate the trades msgs, how much funds to send to what auction.
fn generate_trades_msgs(
    deps: Deps,
    mut to_sell: Vec<TargetHelper>,
    mut to_buy: Vec<TargetHelper>,
    auction_manager: &Addr,
    config: &RebalancerConfig,
    total_value: Decimal,
) -> (Vec<CosmosMsg>, Vec<RebalanceTrade>) {
    let max_trades = to_sell.len().max(to_buy.len());
    let mut msgs: Vec<CosmosMsg> = Vec::with_capacity(max_trades);
    let mut trades: Vec<RebalanceTrade> = Vec::with_capacity(max_trades);

    // Get max tokens to sell as a value and not amount
    let mut max_sell = config.max_limit * total_value;

    // If we have min balance, we need to first check we are not limited by the auction_min_amount
    // Which might prevent us from actually reaching our minimum balance and will always be some tokens short of it.
    // The specific case handled here is when we try to buy a token that has min_balance,
    // but the amount we need to buy is below our auction_min_amount.
    //
    // The main loop below can't handle this case because we first look at the sell amount,
    // and match it to the buy amount, but in this case, we need to do the opposite.
    // This specific case require us to sell more tokens then intended in order to
    // fulfil the min_balance limit.
    //
    // Example:
    // auction_min_amount = 100 utokens, min_balance = X, current balance = X - 50 utokens.
    // In order to reach the minimum balance, we need to buy 50 utokens, but we can't buy less then 100 utokens.
    // On the main loop, this trade will not be executed because of the auction_min_amount.
    //
    // This is not the intented behavior we want, we must fulfull the minimum balance requirement,
    // and to do so, we need to buy the minimum amount we can (100 utokens).
    // Which can't be fully done on the main loop, so we resolve this before that.
    if config.has_min_balance {
        if let Some(token_buy) = to_buy.iter_mut().find(|t| t.target.min_balance.is_some()) {
            // TODO: Should we just take the first sell token? or have some special logic?
            let token_sell = &mut to_sell[0];

            // check if the amount we intent to buy, is lower than min_amount of the sell token
            // if its not, it will be handled correctly by the main loop.
            // but if it is, it means we need to sell other token more then we intent to
            if token_buy.value_to_trade < token_sell.auction_min_send_value {
                // If the amount we try to sell, is below the auction_min_amount, we need to set it to zero
                // else we reduce the auction_min_amount value
                if token_sell.value_to_trade < token_sell.auction_min_send_value {
                    token_sell.value_to_trade = Decimal::zero();
                } else {
                    token_sell.value_to_trade -= token_sell.auction_min_send_value;
                }

                let pair = Pair::from((
                    token_sell.target.denom.clone(),
                    token_buy.target.denom.clone(),
                ));
                let amount = (token_sell.auction_min_send_value * token_sell.price).to_uint_ceil();
                let trade = RebalanceTrade::new(pair, amount);

                token_buy.value_to_trade = Decimal::zero();

                if let Ok(msg) = construct_msg(deps, auction_manager.clone(), trade.clone()) {
                    max_sell -= token_sell.auction_min_send_value;
                    msgs.push(msg);
                    trades.push(trade);
                };
            }
        }
    };

    // This is the main loop where we match to_sell tokens with to_buy tokens
    to_sell.into_iter().for_each(|mut token_sell| {
        to_buy.iter_mut().for_each(|token_buy| {
            // If we already bought all we need for this token we continue to next buy token
            if token_buy.value_to_trade.is_zero() {
                return;
            }

            // If we finished with the sell token, we do nothing
            if token_sell.value_to_trade.is_zero() {
                return;
            }

            // if our max sell is 0, means we sold the max amount the user allowed us, so continue
            if max_sell.is_zero() {
                return;
            }

            let sell_amount = (token_sell.value_to_trade * token_sell.price).to_uint_ceil();

            // Verify we don't sell below min_balance limits
            if let Some(min_balance) = token_sell.target.min_balance {
                if token_sell.balance_amount < sell_amount {
                    // sanity check, make sure we don't try to sell more then we own
                    return;
                } else if token_sell.balance_amount - sell_amount < min_balance {
                    // If our sell results in less then min_balance, we sell the difference to hit min_balance
                    let diff = token_sell.balance_amount - min_balance;

                    if diff.is_zero() {
                        return;
                    }

                    // Unwrap should be safe here because diff should be a small number
                    // and directly related to users balance
                    token_sell.value_to_trade =
                        token_sell.price / Decimal::from_atomics(diff, 0).unwrap();
                }
            }

            // If we intent to sell less then our minimum, we set to_trade to be 0 and continue
            if token_sell.value_to_trade < token_sell.auction_min_send_value {
                token_sell.value_to_trade = Decimal::zero();
                return;
            }

            // If our buy value is lower then our sell min_send value, we do nothing and continue.
            if token_buy.value_to_trade < token_sell.auction_min_send_value {
                return;
            }

            // If we hit our max sell limit, we only sell the limit left
            // otherwise, we keep track of how much we already sold
            if token_sell.value_to_trade > max_sell {
                token_sell.value_to_trade = max_sell;
            }

            let pair = Pair::from((
                token_sell.target.denom.clone(),
                token_buy.target.denom.clone(),
            ));

            if token_sell.value_to_trade >= token_buy.value_to_trade {
                token_sell.value_to_trade -= token_buy.value_to_trade;

                let amount = (token_buy.value_to_trade * token_sell.price).to_uint_ceil();
                let trade = RebalanceTrade::new(pair, amount);

                token_buy.value_to_trade = Decimal::zero();

                let Ok(msg) = construct_msg(deps, auction_manager.clone(), trade.clone()) else {
                    max_sell -= token_buy.value_to_trade;
                    return;
                };

                msgs.push(msg);
                trades.push(trade);
            } else {
                token_buy.value_to_trade -= token_sell.value_to_trade;

                let amount = (token_sell.value_to_trade * token_sell.price).to_uint_ceil();
                let trade = RebalanceTrade::new(pair, amount);

                token_sell.value_to_trade = Decimal::zero();

                let Ok(msg) = construct_msg(deps, auction_manager.clone(), trade.clone()) else {
                    max_sell -= token_sell.value_to_trade;
                    return;
                };

                msgs.push(msg);
                trades.push(trade);
            }
        });
    });

    (msgs, trades)
}
