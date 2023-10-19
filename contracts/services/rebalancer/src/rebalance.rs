use std::{borrow::BorrowMut, str::FromStr};

use auction_package::{helpers::GetPriceResponse, states::MIN_AUCTION_AMOUNT, Pair};
use cosmwasm_std::{
    coin, to_binary, Addr, Coin, CosmosMsg, Decimal, Deps, DepsMut, Env, Order, Response, StdError,
    SubMsg, Uint128, WasmMsg,
};
use cw_storage_plus::Bound;
use valence_package::{
    helpers::start_of_day,
    services::rebalancer::{ParsedPID, RebalancerConfig, TargetOverrideStrategy},
    signed_decimal::SignedDecimal,
};

use crate::{
    contract::{CYCLE_PERIOD, DEFAULT_LIMIT, REPLY_DEFAULT_REBALANCE},
    error::ContractError,
    helpers::{TargetHelper, TradesTuple},
    state::{
        SystemRebalanceStatus, AUCTIONS_MANAGER_ADDR, BASE_DENOM_WHITELIST, CONFIGS,
        DENOM_WHITELIST, SYSTEM_REBALANCE_STATUS,
    },
};

/// Main function for rebalancing using the system
pub fn execute_system_rebalance(
    mut deps: DepsMut,
    env: &Env,
    limit: Option<u64>,
) -> Result<Response, ContractError> {
    // start_from tells us if we should start form a specific addr or from 0
    // cycle_start tells us when the cycle started to calculate for processing and finished status
    let (start_from, cycle_start, prices) = match SYSTEM_REBALANCE_STATUS.load(deps.storage)? {
        SystemRebalanceStatus::NotStarted { cycle_start } => {
            if env.block.time < cycle_start {
                Err(ContractError::CycleNotStartedYet(cycle_start.seconds()))
            } else {
                Ok((None, start_of_day(env.block.time), None))
            }
        }
        SystemRebalanceStatus::Processing {
            cycle_started,
            start_from,
            prices,
        } => {
            if env.block.time > cycle_started.plus_seconds(CYCLE_PERIOD) {
                Ok((None, start_of_day(env.block.time), None))
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

    let mut last_addr = start_from.clone();

    let start_from = start_from.map(Bound::exclusive);
    let limit = limit.unwrap_or(DEFAULT_LIMIT);

    let mut total_accounts: u64 = 0;
    let mut msgs: Vec<SubMsg> = vec![];

    let configs = CONFIGS
        .range(deps.storage, start_from, None, Order::Ascending)
        .take(limit as usize)
        .collect::<Vec<Result<(Addr, RebalancerConfig), StdError>>>();

    let mut min_amount_limits: Vec<(String, Uint128)> = vec![];

    for res in configs {
        total_accounts += 1;

        let Ok((account, config)) = res else {
                  continue;
                };

        last_addr = Some(account.clone());

        // If account paused rebalancing, we continue
        if config.is_paused.is_some() {
            continue;
        };

        // Do rebalance for the account, and construct the msg
        let rebalance_res = do_rebalance(
            deps.as_ref(),
            env,
            &account,
            &auction_manager,
            config,
            &mut min_amount_limits,
            &prices,
        );
        let Ok((config, msg)) = rebalance_res else {
          continue
        };

        // Rebalacing does edit some config fields that are needed for future rebalancing
        // so we save the config here
        CONFIGS.save(deps.branch().storage, account, &config)?;

        msgs.push(msg)
    }

    // We checked if we finished looping over all accounts or not
    // and set the status based on that
    // println!("{total_accounts:?} | {limit:?}");
    let status = if total_accounts < limit {
        SystemRebalanceStatus::Finished {
            next_cycle: cycle_start.plus_seconds(CYCLE_PERIOD),
        }
    } else {
        SystemRebalanceStatus::Processing {
            cycle_started: cycle_start,
            start_from: last_addr.unwrap_or(Addr::unchecked("")),
            prices,
        }
    };

    SYSTEM_REBALANCE_STATUS.save(deps.storage, &status)?;

    Ok(Response::default().add_submessages(msgs))
}

/// Do a rebalance with PID calculation for a single account
pub fn do_rebalance(
    deps: Deps,
    env: &Env,
    account: &Addr,
    auction_manager: &Addr,
    mut config: RebalancerConfig,
    min_amount_limits: &mut Vec<(String, Uint128)>,
    prices: &[(Pair, Decimal)],
) -> Result<(RebalancerConfig, SubMsg), ContractError> {
    // get a vec of inputs for our calculations
    let (total_value, mut target_helpers) = get_inputs(deps, account, &config, prices)?;

    if total_value.is_zero() {
        return Err(ContractError::AccountBalanceIsZero);
    }

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
        diff / Decimal::from_atomics(CYCLE_PERIOD, 0)?
    };

    let (mut to_sell, to_buy) = do_pid(total_value, &mut target_helpers, config.pid.clone(), dt)?;

    // get minimum amount we can send to each auction
    get_auction_min_amount(deps, auction_manager, &mut to_sell, min_amount_limits)?;
    println!("to_sell: {to_sell:?} | to_buy: {to_buy:?}");
    // Generate the trades msgs, how much funds to send to what auction.
    let msgs = generate_trades_msgs(deps, to_sell, to_buy, auction_manager, &config, total_value);

    // Construct the msg we need to execute on the account
    // Notice the atomic false, it means each trade msg (sending funds to specific pair auction)
    // is independent of other trade msg
    // This means 1 trade might fail while another pass, which means rebalance strategy was not executed 100% this cycle
    // but this will be corrected on the next rebalance cycle.
    let msg = SubMsg::reply_on_error(
        WasmMsg::Execute {
            contract_addr: account.to_string(),
            msg: to_binary(
                &valence_package::msgs::core_execute::AccountBaseExecuteMsg::SendFundsByService {
                    msgs,
                    atomic: false,
                },
            )?,
            funds: vec![],
        },
        REPLY_DEFAULT_REBALANCE,
    );

    // We edit config to save data for the next rebalance calculation
    config.last_rebalance = env.block.time;

    Ok((config, msg))
}

/// Get the min amount an auction is willing to accept for a specific token
pub(crate) fn get_auction_min_amount(
    deps: Deps,
    auction_manager: &Addr,
    to_sell: &mut Vec<TargetHelper>,
    min_amount_limits: &mut Vec<(String, Uint128)>,
) -> Result<(), ContractError> {
    for mut sell_token in to_sell {
        match min_amount_limits
            .iter()
            .find(|min_amount| min_amount.0 == sell_token.target.denom)
        {
            Some(min_amount) => {
                sell_token.auction_min_amount =
                    Decimal::from_atomics(min_amount.1, 0)? / sell_token.price;
            }
            None => {
                match MIN_AUCTION_AMOUNT.query(
                    &deps.querier,
                    auction_manager.clone(),
                    sell_token.target.denom.clone(),
                )? {
                    Some(min_amount) => {
                        sell_token.auction_min_amount =
                            Decimal::from_atomics(min_amount, 0)? / sell_token.price;
                        min_amount_limits.push((sell_token.target.denom.clone(), min_amount));
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
            if &base_denom == denom {
                continue;
            }

            let pair = Pair::from((base_denom.clone(), denom.clone()));

            let price = deps.querier.query_wasm_smart::<GetPriceResponse>(
                auctions_manager_addr,
                &auction_package::msgs::AuctionsManagerQueryMsg::GetPrice { pair: pair.clone() },
            )?;

            prices.push((pair, price.price));
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
    // Get the current balances of the account
    let all_balances = deps.querier.query_all_balances(account)?;

    // get inputs per target (denom amount * price),
    // and current total input of the account (vec![denom * price].sum())
    Ok(config.targets.iter().fold(
        (Decimal::zero(), vec![]),
        |(mut total_value, mut targets_helpers), target| {
            // Get the price of the denom, compared to the base denom,
            // if the target denom is the base denom, we set the price to 1
            let price = if target.denom == config.base_denom {
                Decimal::one()
            } else {
                prices
                    .iter()
                    .find(|(pair, _)| pair.1 == target.denom)
                    // we can safely unwrap here as we are 100% sure we have all prices for the whitelisted targets
                    .unwrap()
                    .1
            };

            // Find the target denom in the balance of the account
            // if it doesn't exists, set the input as 0, else set the input as the balance / price
            if let Some(coin) = all_balances.iter().find(|b| b.denom == target.denom) {
                // TODO: Unwrap should be safe here in theory
                let balance_value = Decimal::from_atomics(coin.amount, 0).unwrap() / price;

                total_value += balance_value;
                targets_helpers.push(TargetHelper {
                    target: target.clone(),
                    balance_amount: coin.amount,
                    price,
                    balance_value,
                    value_to_trade: Decimal::zero(),
                    auction_min_amount: Decimal::zero(),
                });

                (total_value, targets_helpers)
            } else {
                targets_helpers.push(TargetHelper {
                    target: target.clone(),
                    balance_amount: Uint128::zero(),
                    price,
                    balance_value: Decimal::zero(),
                    value_to_trade: Decimal::zero(),
                    auction_min_amount: Decimal::zero(),
                });
                (total_value, targets_helpers)
            }
        },
    ))
}

/// Do the PID calculation for the targets
fn do_pid(
    total_value: Decimal,
    targets: &mut [TargetHelper],
    pid: ParsedPID,
    dt: Decimal,
) -> Result<TradesTuple, ContractError> {
    let mut to_sell: Vec<TargetHelper> = vec![];
    let mut to_buy: Vec<TargetHelper> = vec![];

    // turn values into signed decimals
    let signed_p: SignedDecimal = pid.p.into();
    let signed_i: SignedDecimal = pid.i.into();
    let signed_d: SignedDecimal = pid.d.into();

    let signed_dt: SignedDecimal = dt.into();

    println!("total_value: {total_value}");

    targets.iter_mut().for_each(|target| {
        let signed_input: SignedDecimal = target.balance_value.into();

        // Reset to trade value
        target.value_to_trade = Decimal::zero();

        let target_value = SignedDecimal::from(total_value * target.target.percentage);

        let error = target_value - signed_input;

        let p = error * signed_p;
        let i = target.target.last_i + (error * signed_i * signed_dt);
        let mut d = match target.target.last_input {
            Some(last_input) => signed_input - last_input.into(),
            None => SignedDecimal::zero(),
        };
        d = d * signed_d / signed_dt;

        let output = p + i - d;

        target.value_to_trade = output.0;

        target.target.last_input = Some(target.balance_value);
        target.target.last_i = i;

        match output.1 {
            // output is negative, we need to sell
            false => to_sell.push(target.clone()),
            // output is positive, we need to buy
            true => to_buy.push(target.clone()),
        }
    });

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

    let min_balance = Decimal::from_atomics(target.target.min_balance.unwrap(), 0)?;
    let min_balance_target = min_balance * target.price;
    let real_target = total_value * target.target.percentage;

    // if the target is below the minimum balance target
    let new_targets = if real_target < min_balance_target {
        // the target is below min_balance, so we set the min_balance as the new target

        // Verify that min_balance is not higher then our total value, if it is, then we sell everything to fulfill it.
        // println!("{min_balance_input:?} | {total_value:?}");
        let (new_target_perc, mut leftover_perc) = if min_balance_target >= total_value {
            (Decimal::one(), Decimal::zero())
        } else {
            let perc = min_balance_target / total_value;
            (perc, Decimal::one() - perc)
        };
        // println!("{new_target_perc:?} | {leftover_perc:?}");

        let old_leftover_perc = Decimal::one() - target.target.percentage;
        let mut new_total_perc = new_target_perc;

        let updated_targets = targets
            .into_iter()
            .map(|mut t| {
                // If our target is the min_balance target, we update perc, and return t.
                if t.target.denom == target.target.denom {
                    t.target.percentage = new_target_perc;
                    return t;
                };

                // If leftover perc is 0, we set the perc as zero for this target
                if leftover_perc.is_zero() {
                    t.target.percentage = Decimal::zero();
                    return t;
                }

                // Calc new perc based on chosen strategy and new min_balance perc
                match config.target_override_strategy {
                    TargetOverrideStrategy::Proportional => {
                        let old_perc = t.target.percentage / old_leftover_perc;
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
                t
            })
            .collect();

        // If the new percentage is smaller then 0.9999 or higher then 1, we have something wrong in calculation
        if new_total_perc > Decimal::one() || new_total_perc < Decimal::from_str("0.9999")? {
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
    _deps: Deps,
    auction_manager: &Addr,
    pair: Pair,
    amount: Coin,
) -> Result<CosmosMsg, ContractError> {
    let msg = WasmMsg::Execute {
        contract_addr: auction_manager.to_string(),
        msg: to_binary(&auctions_manager::msg::ExecuteMsg::AuctionFunds { pair })?,
        funds: vec![amount],
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
) -> Vec<CosmosMsg> {
    let mut msgs: Vec<CosmosMsg> = vec![];

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
            if token_buy.value_to_trade < token_sell.auction_min_amount {
                // If the amount we try to sell, is below the auction_min_amount, we need to set it to zero
                // else we reduce the auction_min_amount value
                if token_sell.value_to_trade < token_sell.auction_min_amount {
                    token_sell.value_to_trade = Decimal::zero();
                } else {
                    token_sell.value_to_trade -= token_sell.auction_min_amount;
                }

                let pair = Pair::from((
                    token_sell.target.denom.clone(),
                    token_buy.target.denom.clone(),
                ));
                let amount = (token_sell.auction_min_amount * token_sell.price).to_uint_ceil();
                let coin = coin(amount.u128(), &pair.0);

                token_buy.value_to_trade = Decimal::zero();

                if let Ok(msg) = construct_msg(deps, auction_manager, pair, coin) {
                    max_sell -= token_sell.auction_min_amount;
                    msgs.push(msg);
                };
            }
        }
    };

    // This is the main loop where we match to_sell tokens with to_buy tokens
    to_sell.into_iter().for_each(|mut token_sell| {
        to_buy.iter_mut().for_each(|mut token_buy| {
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

                    token_sell.value_to_trade =
                        token_sell.price / Decimal::from_atomics(diff, 0).unwrap();
                }
            }

            // If we intent to sell less then our minimum, we set to_trade to be 0 and continue
            if token_sell.value_to_trade < token_sell.auction_min_amount {
                token_sell.value_to_trade = Decimal::zero();
                return;
            }

            // If we hit our max sell limit, we only sell the limit left
            // otherwise, we keep track of how much we already sold
            if token_sell.value_to_trade > max_sell {
                token_sell.value_to_trade = max_sell;
                max_sell = Decimal::zero();
            } else {
                max_sell -= token_sell.value_to_trade;
            }

            let pair = Pair::from((
                token_sell.target.denom.clone(),
                token_buy.target.denom.clone(),
            ));

            if token_sell.value_to_trade >= token_buy.value_to_trade {
                token_sell.value_to_trade -= token_buy.value_to_trade;

                let coin = coin(
                    (token_buy.value_to_trade * token_sell.price)
                        .to_uint_ceil()
                        .u128(),
                    token_sell.target.denom.clone(),
                );

                token_buy.value_to_trade = Decimal::zero();

                let Ok(msg) = construct_msg(deps, auction_manager, pair, coin) else {
                    return
                };
                msgs.push(msg);
            } else {
                token_buy.value_to_trade -= token_sell.value_to_trade;

                let coin = coin(
                    (token_sell.value_to_trade * token_sell.price)
                        .to_uint_ceil()
                        .u128(),
                    token_sell.target.denom.clone(),
                );

                token_sell.value_to_trade = Decimal::zero();

                let Ok(msg) = construct_msg(deps, auction_manager, pair, coin) else {
                  return
                };

                msgs.push(msg);
            }
        });
    });

    msgs
}