use std::str::FromStr;

use auction_package::{
    error::AuctionError,
    helpers::{ChainHaltConfig, GetPriceResponse},
    Pair, PriceFreshnessStrategy,
};
use cosmwasm_std::{coins, Addr, Decimal};
use cw_multi_test::Executor;
use cw_utils::Expiration;

use crate::suite::{
    instantiates::AuctionInstantiate,
    suite::{Suite, DEFAULT_BLOCK_TIME},
    suite_builder::SuiteBuilder,
};

#[test]
fn test_pause_auction() {
    let mut suite = Suite::default();

    suite.pause_auction(suite.pair.clone());

    let config = suite.query_auction_config(suite.get_default_auction_addr());
    assert!(config.is_paused)
}

#[test]
fn test_resume_auction() {
    let mut suite = Suite::default();

    suite.pause_auction(suite.pair.clone());

    suite.resume_auction(suite.pair.clone()).unwrap();

    let config = suite.query_auction_config(suite.get_default_auction_addr());
    assert!(!config.is_paused)
}

#[test]
fn test_update_oracle_addr() {
    let mut suite = Suite::default();
    let new_oracle_addr = "new_oracle_addr";

    suite.update_oracle(new_oracle_addr);

    let addr = suite.query_oracle_addr();
    assert_eq!(addr.as_str(), new_oracle_addr);
}

#[test]
fn test_auction_funds_from_manager() {
    let mut suite = Suite::default();
    let amount = coins(100u128, suite.pair.0.clone());

    suite.auction_funds_manager(suite.pair.clone(), suite.get_account_addr(0), &amount);

    let funds_res =
        suite.query_auction_funds(suite.get_account_addr(0), suite.get_default_auction_addr());
    assert_eq!(funds_res.next, amount[0].amount);
}

#[test]
fn test_not_admin() {
    let mut suite = Suite::default();

    let err = suite
        .app
        .execute_contract(
            Addr::unchecked("not_admin"),
            suite.auctions_manager_addr,
            &auctions_manager::msg::ExecuteMsg::Admin(Box::new(
                auctions_manager::msg::AdminMsgs::PauseAuction { pair: suite.pair },
            )),
            &[],
        )
        .unwrap_err();
    let err = err.source().unwrap().to_string();

    assert_eq!(err, AuctionError::NotAdmin.to_string());
}

#[test]
fn test_no_oracle_addr() {
    let mut suite = SuiteBuilder::default()
        .without_oracle_addr()
        .build_basic(true);
    let pair = Pair::from(("random".to_string(), "random2".to_string()));

    suite.init_auction(pair.clone(), AuctionInstantiate::default().into(), None);

    let err = suite
        .app
        .wrap()
        .query_wasm_smart::<GetPriceResponse>(
            suite.auctions_manager_addr,
            &auction_package::msgs::AuctionsManagerQueryMsg::GetPrice { pair },
        )
        .unwrap_err();

    assert!(err
        .to_string()
        .contains(&auctions_manager::error::ContractError::OracleAddrMissing.to_string()));
}

#[test]
fn test_update_chain_halt_config() {
    let mut suite = Suite::default();

    let config = suite.query_auction_config(suite.get_default_auction_addr());
    assert_eq!(
        config.chain_halt_config,
        ChainHaltConfig {
            cap: 60 * 60 * 4,
            block_avg: Decimal::from_str(&DEFAULT_BLOCK_TIME.to_string()).unwrap(),
        }
    );

    let new_chain_halt_config = ChainHaltConfig {
        cap: 60 * 60 * 10,
        block_avg: Decimal::from_str("2").unwrap(),
    };
    suite.update_chain_halt_config(suite.pair.clone(), new_chain_halt_config.clone());

    let config = suite.query_auction_config(suite.get_default_auction_addr());
    assert_eq!(config.chain_halt_config, new_chain_halt_config);
}

#[test]
fn test_update_price_freshness_strategy() {
    let mut suite = Suite::default();

    let config = suite.query_auction_config(suite.get_default_auction_addr());
    assert_eq!(
        config.price_freshness_strategy,
        PriceFreshnessStrategy {
            limit: Decimal::from_str("3").unwrap(),
            multipliers: vec![
                // If older than 2 days, multiplier is 2x the strategy multiplier
                (
                    Decimal::from_str("2").unwrap(),
                    Decimal::from_str("2").unwrap(),
                ),
                // If older than 1 days, multiplier is 1.5x the strategy multiplier
                (Decimal::one(), Decimal::from_str("1.5").unwrap()),
            ],
        }
    );

    let new_price_freshness_strategy = PriceFreshnessStrategy {
        limit: Decimal::from_str("4").unwrap(),
        multipliers: vec![
            // If older than 2 days, multiplier is 2x the strategy multiplier
            (
                Decimal::from_str("3").unwrap(),
                Decimal::from_str("2").unwrap(),
            ),
            (
                Decimal::from_str("2").unwrap(),
                Decimal::from_str("1.6").unwrap(),
            ),
            // If older than 1 days, multiplier is 1.5x the strategy multiplier
            (Decimal::one(), Decimal::from_str("1.3").unwrap()),
        ],
    };
    suite.update_price_freshness_strategy(suite.pair.clone(), new_price_freshness_strategy.clone());

    let config = suite.query_auction_config(suite.get_default_auction_addr());
    assert_eq!(
        config.price_freshness_strategy,
        new_price_freshness_strategy
    );
}

#[test]
fn test_update_admin_start() {
    let mut suite = Suite::default();
    let new_admin = Addr::unchecked("new_admin_addr");

    // Try to approve admin without starting a new change
    // should error
    suite
        .app
        .execute_contract(
            new_admin.clone(),
            suite.auctions_manager_addr.clone(),
            &price_oracle::msg::ExecuteMsg::ApproveAdminChange {},
            &[],
        )
        .unwrap_err();

    suite
        .app
        .execute_contract(
            suite.admin.clone(),
            suite.auctions_manager_addr.clone(),
            &auctions_manager::msg::ExecuteMsg::Admin(Box::new(
                auctions_manager::msg::AdminMsgs::StartAdminChange {
                    addr: new_admin.to_string(),
                    expiration: Expiration::Never {},
                },
            )),
            &[],
        )
        .unwrap();

    suite
        .app
        .execute_contract(
            new_admin.clone(),
            suite.auctions_manager_addr.clone(),
            &price_oracle::msg::ExecuteMsg::ApproveAdminChange {},
            &[],
        )
        .unwrap();

    let admin = suite.query_admin(&suite.auctions_manager_addr).unwrap();
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
            suite.auctions_manager_addr.clone(),
            &auctions_manager::msg::ExecuteMsg::Admin(Box::new(
                auctions_manager::msg::AdminMsgs::StartAdminChange {
                    addr: new_admin.to_string(),
                    expiration: Expiration::Never {},
                },
            )),
            &[],
        )
        .unwrap();

    suite
        .app
        .execute_contract(
            suite.admin.clone(),
            suite.auctions_manager_addr.clone(),
            &auctions_manager::msg::ExecuteMsg::Admin(Box::new(
                auctions_manager::msg::AdminMsgs::CancelAdminChange {},
            )),
            &[],
        )
        .unwrap();

    // Should error because we cancelled the admin change
    suite
        .app
        .execute_contract(
            new_admin,
            suite.auctions_manager_addr.clone(),
            &auctions_manager::msg::ExecuteMsg::ApproveAdminChange {},
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
            suite.auctions_manager_addr.clone(),
            &auctions_manager::msg::ExecuteMsg::Admin(Box::new(
                auctions_manager::msg::AdminMsgs::StartAdminChange {
                    addr: new_admin.to_string(),
                    expiration: Expiration::AtHeight(suite.app.block_info().height + 5),
                },
            )),
            &[],
        )
        .unwrap();

    // Should fail because we are not the new admin
    suite
        .app
        .execute_contract(
            random_addr,
            suite.auctions_manager_addr.clone(),
            &auctions_manager::msg::ExecuteMsg::ApproveAdminChange {},
            &[],
        )
        .unwrap_err();

    suite.update_block_cycle();

    // Should fail because expired
    suite
        .app
        .execute_contract(
            new_admin,
            suite.auctions_manager_addr.clone(),
            &auctions_manager::msg::ExecuteMsg::ApproveAdminChange {},
            &[],
        )
        .unwrap_err();
}

#[test]
fn test_change_server_addr() {
    let mut suite = Suite::default();
    let new_server_addr = Addr::unchecked("new_server_addr");

    suite.change_server_addr(new_server_addr.clone());

    let server_addr = suite.query_server_addr();
    assert_eq!(server_addr, new_server_addr);
}
