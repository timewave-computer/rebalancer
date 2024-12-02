use std::borrow::BorrowMut;

use cosmwasm_std::{coins, Addr, Decimal};
use cw_multi_test::Executor;
use cw_utils::Expiration;
use price_oracle::state::PriceStep;

use crate::suite::{
    suite::{Suite, DAY, DEFAULT_BLOCK_TIME},
    suite_builder::SuiteBuilder,
};

#[test]
fn test_update_price_manually() {
    let mut suite = SuiteBuilder::default().build_basic(false);

    let price = Decimal::bps(5000);
    suite
        .manual_update_price(suite.pair.clone(), price)
        .unwrap();

    let price_res = suite.query_oracle_price(suite.pair.clone());
    assert_eq!(price_res.price, price);
}

#[test]
fn test_update_price_from_auctions() {
    let mut suite = Suite::default();
    let funds = coins(100_u128, suite.pair.0.clone());

    // do 3 auctions
    suite.finalize_auction(&funds);
    suite.finalize_auction(&funds);
    suite.finalize_auction(&funds);

    // Update the price from twap
    suite.update_price(suite.pair.clone()).unwrap();

    // Get the price which should be an average of 1.5
    let price_res = suite.query_oracle_price(suite.pair.clone());
    let rounded_price =
        (price_res.price * Decimal::from_atomics(100_u128, 0).unwrap()).to_uint_floor();
    assert_eq!(rounded_price.u128(), 150_u128); // 150 / 100 = 1.50
}

// TODO: Should fallback to astroport and not error, remove once astroport test is added
#[test]
fn test_twap_less_then_3_auctions() {
    let mut suite = Suite::default();
    let funds = coins(1000_u128, suite.pair.0.clone());

    // do auction
    suite.finalize_auction(&funds);

    let _ = suite.update_price_err(suite.pair.clone());
    // assert_eq!(err, price_oracle::error::ContractError::NotEnoughTwaps)
}

// NOTE: This doesn't actually test
#[test]
fn test_twap_no_recent_auction() {
    let mut suite = Suite::default();
    let funds = coins(100_u128, suite.pair.0.clone());

    // do 3 auctions
    suite.finalize_auction(&funds);
    suite.finalize_auction(&funds);
    suite.finalize_auction(&funds);

    // Move chain 6 days ahead
    suite.update_block(DAY * 4 / DEFAULT_BLOCK_TIME);

    let err = suite.update_price_err(suite.pair.clone());
    assert_eq!(
        err,
        price_oracle::error::ContractError::NoAstroPath(suite.pair.clone())
    )
}

#[test]
fn test_not_admin() {
    let mut suite = Suite::default();
    let funds = coins(100_u128, suite.pair.0.clone());

    // do 3 auctions
    suite.finalize_auction(&funds);
    suite.finalize_auction(&funds);
    suite.finalize_auction(&funds);

    // Should error because we are not the admin
    suite
        .app
        .execute_contract(
            Addr::unchecked("not_admin"),
            suite.oracle_addr,
            &price_oracle::msg::ExecuteMsg::ManualPriceUpdate {
                pair: suite.pair,
                price: Decimal::one(),
            },
            &[],
        )
        .unwrap_err();
}

#[test]
fn test_config() {
    let suite = Suite::default();
    let config = suite.query_oracle_config();
    assert_eq!(
        config,
        price_oracle::state::Config {
            auction_manager_addr: suite.auctions_manager_addr,
            seconds_allow_manual_change: 60 * 60 * 24 * 2,
            seconds_auction_prices_fresh: 60 * 60 * 24 * 3,
        }
    )
}

#[test]
fn test_update_price_0() {
    let mut suite = SuiteBuilder::default().build_basic(true);

    let price: Decimal = Decimal::zero();
    let err = suite.manual_update_price_err(suite.pair.clone(), price);

    assert_eq!(err, price_oracle::error::ContractError::PriceIsZero)
}

#[test]
fn test_update_admin_start() {
    let mut suite = Suite::default();
    let new_admin = Addr::unchecked("random_addr");

    // Try to approve admin without starting a new change
    // should error
    suite
        .app
        .execute_contract(
            new_admin.clone(),
            suite.oracle_addr.clone(),
            &price_oracle::msg::ExecuteMsg::ApproveAdminChange {},
            &[],
        )
        .unwrap_err();

    suite
        .app
        .execute_contract(
            suite.admin.clone(),
            suite.oracle_addr.clone(),
            &price_oracle::msg::ExecuteMsg::StartAdminChange {
                addr: new_admin.to_string(),
                expiration: Expiration::Never {},
            },
            &[],
        )
        .unwrap();

    suite
        .app
        .execute_contract(
            new_admin.clone(),
            suite.oracle_addr.clone(),
            &price_oracle::msg::ExecuteMsg::ApproveAdminChange {},
            &[],
        )
        .unwrap();

    let admin = suite.query_admin(&suite.oracle_addr).unwrap();
    assert_eq!(admin, new_admin)
}

#[test]
fn test_update_admin_cancel() {
    let mut suite = Suite::default();
    let new_admin = Addr::unchecked("new_admin_addr");

    suite
        .app
        .execute_contract(
            suite.admin.clone(),
            suite.oracle_addr.clone(),
            &price_oracle::msg::ExecuteMsg::StartAdminChange {
                addr: new_admin.to_string(),
                expiration: Expiration::Never {},
            },
            &[],
        )
        .unwrap();

    suite
        .app
        .execute_contract(
            suite.admin.clone(),
            suite.oracle_addr.clone(),
            &price_oracle::msg::ExecuteMsg::CancelAdminChange {},
            &[],
        )
        .unwrap();

    // Should error because we cancelled the admin change
    suite
        .app
        .execute_contract(
            new_admin,
            suite.oracle_addr.clone(),
            &price_oracle::msg::ExecuteMsg::ApproveAdminChange {},
            &[],
        )
        .unwrap_err();
}

#[test]
fn test_update_admin_fails() {
    let mut suite = Suite::default();
    let new_admin = Addr::unchecked("new_admin_addr");
    let random_addr = Addr::unchecked("random_addr");

    suite
        .app
        .execute_contract(
            suite.admin.clone(),
            suite.oracle_addr.clone(),
            &price_oracle::msg::ExecuteMsg::StartAdminChange {
                addr: new_admin.to_string(),
                expiration: Expiration::AtHeight(suite.app.block_info().height + 5),
            },
            &[],
        )
        .unwrap();

    // Should fail because we are not the new admin
    suite
        .app
        .execute_contract(
            random_addr,
            suite.oracle_addr.clone(),
            &price_oracle::msg::ExecuteMsg::ApproveAdminChange {},
            &[],
        )
        .unwrap_err();

    suite.update_block_cycle();

    // Should fail because expired
    suite
        .app
        .execute_contract(
            new_admin,
            suite.oracle_addr.clone(),
            &price_oracle::msg::ExecuteMsg::ApproveAdminChange {},
            &[],
        )
        .unwrap_err();
}

#[test]
fn test_manual_price_update() {
    let mut suite = SuiteBuilder::default().build_basic(false);
    let funds = coins(10_u128, suite.pair.0.clone());

    // no auctions yet, so should be able to update
    suite
        .manual_update_price(suite.pair.clone(), Decimal::one())
        .unwrap();

    // 4 auctions passed we should not be able to update price now.
    suite.finalize_auction(&funds);
    suite.finalize_auction(&funds);
    suite.finalize_auction(&funds);
    suite.finalize_auction(&funds);

    suite.update_price(suite.pair.clone()).unwrap();

    let err = suite.manual_update_price_err(suite.pair.clone(), Decimal::one());
    assert_eq!(
        err,
        price_oracle::error::ContractError::NoTermsForManualUpdate
    );

    // 3 days passed without auction, we should be able to update price now.
    suite.update_block_cycle();
    suite.update_block_cycle();
    suite.update_block_cycle();

    suite
        .manual_update_price(suite.pair.clone(), Decimal::one())
        .unwrap();

    // an auction happened, we should not be able to update price now.
    suite.finalize_auction(&funds);

    let err = suite.manual_update_price_err(suite.pair.clone(), Decimal::one());
    assert_eq!(
        err,
        price_oracle::error::ContractError::NoTermsForManualUpdate
    );
}

#[test]
fn test_update_config() {
    let mut suite = Suite::default();

    let oracle_config = suite.query_oracle_config();

    let new_addr = "some_addr";

    suite
        .app
        .execute_contract(
            suite.admin.clone(),
            suite.oracle_addr.clone(),
            &price_oracle::msg::ExecuteMsg::UpdateConfig {
                auction_manager_addr: Some(new_addr.to_string()),
                seconds_allow_manual_change: Some(12),
                seconds_auction_prices_fresh: Some(455),
            },
            &[],
        )
        .unwrap();

    let new_oracle_config = suite.query_oracle_config();

    assert_ne!(new_oracle_config, oracle_config);
    assert_eq!(new_oracle_config.auction_manager_addr, new_addr.to_string());
    assert_eq!(new_oracle_config.seconds_allow_manual_change, 12);
    assert_eq!(new_oracle_config.seconds_auction_prices_fresh, 455);
}

#[test]
fn test_local_prices() {
    let mut suite = Suite::default();
    let funds = coins(100_u128, suite.pair.0.clone());

    // do 3 auctions
    suite.finalize_auction(&funds);
    suite.finalize_auction(&funds);
    suite.finalize_auction(&funds);

    // Update the price from auction
    suite.update_price(suite.pair.clone()).unwrap();

    // Register the astro path
    let path = vec![PriceStep {
        denom1: suite.pair.0.to_string(),
        denom2: suite.pair.1.to_string(),
        pool_address: suite
            .astro_pools
            .get(&suite.pair.clone().into())
            .unwrap()
            .clone(),
    }];
    suite
        .add_astro_path_to_oracle(suite.pair.clone(), path)
        .unwrap();

    // Randomize the pool a little to get a "nice" price
    let mut rng = rand::thread_rng();

    for _ in 0..10 {
        suite.do_random_swap(
            rng.borrow_mut(),
            suite.pair.clone(),
            100_000_u128,
            1_000_000_u128,
        );
    }

    let pool_price = suite.query_astro_pool_price(
        suite
            .astro_pools
            .get(&suite.pair.clone().into())
            .unwrap()
            .clone(),
        suite.pair.clone(),
    );

    // make sure the last price on local, is not the price from the pool
    let local_prices = suite.query_oracle_local_price(suite.pair.clone());
    assert_ne!(local_prices[0].price, pool_price);

    // Update price, should get it from astro
    suite.update_block(DAY / DEFAULT_BLOCK_TIME * 2);

    // Update the price from auction
    suite.update_price(suite.pair.clone()).unwrap();

    let local_prices = suite.query_oracle_local_price(suite.pair.clone());

    // We should have 2 prices because we updated prices twice
    assert_eq!(local_prices.len(), 3);
    // The first price should be the pool price
    assert_eq!(local_prices[0].price, pool_price);

    // make sure the actual price is the avg of the local prices
    let oracle_price = suite.query_oracle_price(suite.pair.clone());
    assert_eq!(
        oracle_price.price,
        (local_prices[0].price + local_prices[1].price + local_prices[2].price)
            / Decimal::from_atomics(3_u128, 0).unwrap()
    );
}
