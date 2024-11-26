use std::collections::HashSet;

use auction_package::Pair;
use cosmwasm_std::{testing::mock_env, Addr, BlockInfo, Decimal, Empty, Timestamp};
use cw_multi_test::Executor;
use cw_utils::Expiration;
use rebalancer::contract::DEFAULT_CYCLE_PERIOD;
use valence_package::{
    error::ValenceError,
    helpers::start_of_cycle,
    services::{
        rebalancer::{SystemRebalanceStatus, Target},
        ValenceServices,
    },
};

use crate::suite::{
    instantiates::RebalancerInstantiate,
    suite::{
        Suite, ATOM, DEFAULT_BLOCK_TIME, DEFAULT_NTRN_PRICE_BPS, DEFAULT_OSMO_PRICE_BPS, NTRN, OSMO,
    },
    suite_builder::SuiteBuilder,
};

#[test]
fn test_rebalancer_system() {
    let mut suite = SuiteBuilder::default().with_accounts(2).build_default();

    // First confirm status is not started
    let status = suite.query_rebalancer_system_status().unwrap();
    assert_eq!(
        status,
        SystemRebalanceStatus::NotStarted {
            cycle_start: mock_env().block.time
        }
    );

    // Do a rebalance with limit of 1 to go into the processing status
    suite.rebalance_with_update_block(Some(1)).unwrap();

    // check status matches what we expect
    let SystemRebalanceStatus::Processing {
        cycle_started,
        start_from,
        prices,
    } = suite.query_rebalancer_system_status().unwrap()
    else {
        panic!("System status is not processing but something else")
    };
    assert_eq!(
        cycle_started,
        start_of_cycle(suite.app.block_info().time, DEFAULT_CYCLE_PERIOD)
    );
    assert_eq!(start_from, suite.get_account_addr(0));
    assert!(prices.contains(&(
        Pair::from((ATOM.to_string(), NTRN.to_string())),
        Decimal::bps(DEFAULT_NTRN_PRICE_BPS)
    )));
    assert!(prices.contains(&(
        Pair::from((ATOM.to_string(), OSMO.to_string())),
        Decimal::bps(DEFAULT_OSMO_PRICE_BPS)
    )));
    assert!(prices.contains(&(
        Pair::from((NTRN.to_string(), ATOM.to_string())),
        Decimal::one() / Decimal::bps(DEFAULT_NTRN_PRICE_BPS)
    )));
    assert!(prices.contains(&(
        Pair::from((NTRN.to_string(), OSMO.to_string())),
        Decimal::bps(DEFAULT_OSMO_PRICE_BPS) / Decimal::bps(DEFAULT_NTRN_PRICE_BPS)
    )));

    // confirm our rebalancer ran over the first account only
    let config_1 = suite
        .query_rebalancer_config(suite.get_account_addr(0))
        .unwrap();
    let config_2 = suite
        .query_rebalancer_config(suite.get_account_addr(1))
        .unwrap();

    assert_eq!(config_1.last_rebalance, suite.app.block_info().time);
    assert_eq!(config_2.last_rebalance, Timestamp::from_seconds(0));

    // Do a final Rebalance
    suite.add_block();
    suite.rebalance(Some(2)).unwrap();

    // Check status is Finished
    let status = suite.query_rebalancer_system_status().unwrap();
    assert_eq!(
        status,
        SystemRebalanceStatus::Finished {
            next_cycle: start_of_cycle(suite.app.block_info().time, DEFAULT_CYCLE_PERIOD)
                .plus_seconds(DEFAULT_CYCLE_PERIOD)
        }
    );

    let config_1 = suite
        .query_rebalancer_config(suite.get_account_addr(0))
        .unwrap();
    let config_2 = suite
        .query_rebalancer_config(suite.get_account_addr(1))
        .unwrap();

    assert_eq!(
        config_1.last_rebalance,
        suite
            .app
            .block_info()
            .time
            .minus_seconds(DEFAULT_BLOCK_TIME)
    );
    assert_eq!(config_2.last_rebalance, suite.app.block_info().time);

    // Try to do another rebalance today, should fail
    suite.add_block();
    let err: rebalancer::error::ContractError =
        suite.rebalance(Some(2)).unwrap_err().downcast().unwrap();

    assert_eq!(
        err,
        rebalancer::error::ContractError::CycleNotStartedYet(
            start_of_cycle(
                suite
                    .app
                    .block_info()
                    .time
                    .plus_seconds(DEFAULT_CYCLE_PERIOD),
                DEFAULT_CYCLE_PERIOD
            )
            .seconds()
        )
    )
}

#[test]
fn test_register() {
    let mut suite = SuiteBuilder::default().with_accounts(2).build_basic(true);

    // Because we have a basic setup here, we need to register the service to the manager
    suite
        .add_service_to_manager(
            suite.admin.clone(),
            suite.manager_addr.clone(),
            ValenceServices::Rebalancer,
            suite.rebalancer_addr.to_string(),
        )
        .unwrap();

    suite
        .register_to_rebalancer(0, &SuiteBuilder::get_default_rebalancer_register_data())
        .unwrap();

    // Try to register when already registered
    let err =
        suite.register_to_rebalancer_err(0, &SuiteBuilder::get_default_rebalancer_register_data());
    assert_eq!(
        err,
        rebalancer::error::ContractError::AccountAlreadyRegistered
    );

    // Try register with not whitelisted base denom
    let mut register_data = SuiteBuilder::get_default_rebalancer_register_data();
    register_data.base_denom = "not_whitelisted_denom".to_string();

    let err = suite.register_to_rebalancer_err(1, &register_data);
    assert_eq!(
        err,
        rebalancer::error::ContractError::BaseDenomNotWhitelisted(
            "not_whitelisted_denom".to_string()
        )
    );

    // Try to register with only 1 target
    register_data = SuiteBuilder::get_default_rebalancer_register_data();
    let mut targets = HashSet::with_capacity(1);
    targets.insert(Target {
        denom: ATOM.to_string(),
        bps: 10000,
        min_balance: None,
    });
    register_data.targets = targets.clone();

    let err = suite.register_to_rebalancer_err(1, &register_data);
    assert_eq!(err, rebalancer::error::ContractError::TwoTargetsMinimum);

    // Try to register with not whitelisted denom
    register_data = SuiteBuilder::get_default_rebalancer_register_data();
    targets.clear();
    targets.insert(Target {
        denom: ATOM.to_string(),
        bps: 5000,
        min_balance: None,
    });
    targets.insert(Target {
        denom: "not_whitelisted_denom".to_string(),
        bps: 5000,
        min_balance: None,
    });

    register_data.targets = targets.clone();

    let err = suite.register_to_rebalancer_err(1, &register_data);
    assert_eq!(
        err,
        rebalancer::error::ContractError::DenomNotWhitelisted("not_whitelisted_denom".to_string())
    );

    // Try to register with wrong total percentage (must equal 10000)
    register_data = SuiteBuilder::get_default_rebalancer_register_data();
    targets.clear();
    targets.insert(Target {
        denom: ATOM.to_string(),
        bps: 6000,
        min_balance: None,
    });
    targets.insert(Target {
        denom: NTRN.to_string(),
        bps: 5000,
        min_balance: None,
    });
    register_data.targets = targets;

    let err = suite.register_to_rebalancer_err(1, &register_data);
    assert_eq!(
        err,
        rebalancer::error::ContractError::InvalidTargetPercentage("11000".to_string())
    );
}

#[test]
fn test_dup_targets() {
    let mut suite = SuiteBuilder::default().with_accounts(1).build_basic(true);

    suite
        .add_service_to_manager(
            suite.admin.clone(),
            suite.manager_addr.clone(),
            ValenceServices::Rebalancer,
            suite.rebalancer_addr.to_string(),
        )
        .unwrap();

    let mut register_data = SuiteBuilder::get_default_rebalancer_register_data();
    register_data.targets.insert(
        register_data
            .targets
            .iter()
            .find(|t| t.denom == ATOM)
            .unwrap()
            .clone(),
    );
    assert!(register_data.targets.len() == 2);

    // We try to insert different struct, with the the same denom
    let mut new_target = register_data
        .targets
        .iter()
        .find(|t| t.denom == ATOM)
        .unwrap()
        .clone();
    new_target.bps = 1;

    register_data.targets.insert(new_target);

    assert!(register_data.targets.len() == 2);
}

#[test]
fn test_set_2_min_balance() {
    let mut suite = SuiteBuilder::default().build_basic(true);

    suite
        .add_service_to_manager(
            suite.admin.clone(),
            suite.manager_addr.clone(),
            ValenceServices::Rebalancer,
            suite.rebalancer_addr.to_string(),
        )
        .unwrap();

    let mut register_data = SuiteBuilder::get_default_rebalancer_register_data();

    // set both to have min_balance
    let mut targets = SuiteBuilder::get_default_targets();
    targets[0].min_balance = Some(100_u128.into());
    targets[1].min_balance = Some(100_u128.into());

    register_data.targets = HashSet::from_iter(targets.iter().cloned());

    let err = suite.register_to_rebalancer_err(0, &register_data);
    assert_eq!(
        err,
        rebalancer::error::ContractError::MultipleMinBalanceTargets
    );
}

#[test]
fn test_no_balance() {
    let mut suite = SuiteBuilder::default().build_default();
    let acc_addr = suite.get_account_addr(0);
    suite.app.init_modules(|router, _, storage| {
        router
            .bank
            .init_balance(storage, &acc_addr, vec![])
            .unwrap();
    });

    // Should not error and basically do nothing.
    suite.resolve_cycle();
}

#[test]
fn test_rebalancer_cycle_before() {
    let mut suite = SuiteBuilder::default().build_default();

    // Set block to before mock_env.
    suite.app.set_block(BlockInfo {
        height: mock_env().block.height - 2,
        time: mock_env().block.time.minus_seconds(12),
        chain_id: "".to_string(),
    });

    let err: rebalancer::error::ContractError =
        suite.rebalance(None).unwrap_err().downcast().unwrap();
    assert_eq!(
        err,
        rebalancer::error::ContractError::CycleNotStartedYet(mock_env().block.time.seconds())
    )
}

#[test]
fn test_rebalancer_cycle_next_day_while_processing() {
    let mut suite = SuiteBuilder::default().with_accounts(2).build_default();

    // Do only rebalance for 1 account
    suite.rebalance_with_update_block(Some(1)).unwrap();

    // Make sure last rebalance for 2nd account is 0
    let config = suite
        .query_rebalancer_config(suite.get_account_addr(1))
        .unwrap();
    assert!(config.last_rebalance.seconds() == 0);

    // Do another rebalance for both accounts now.
    let res = suite.rebalance_with_update_block(None).unwrap();
    println!("{:?}", res);

    let config1 = suite
        .query_rebalancer_config(suite.get_account_addr(1))
        .unwrap();
    let config2 = suite
        .query_rebalancer_config(suite.get_account_addr(1))
        .unwrap();
    assert_eq!(config1.last_rebalance, suite.app.block_info().time);
    assert_eq!(config2.last_rebalance, suite.app.block_info().time);
}

#[test]
fn test_invalid_max_limit_range() {
    let mut suite = SuiteBuilder::default().with_accounts(2).build_basic(true);

    // Because we have a basic setup here, we need to register the service to the manager
    suite
        .add_service_to_manager(
            suite.admin.clone(),
            suite.manager_addr.clone(),
            ValenceServices::Rebalancer,
            suite.rebalancer_addr.to_string(),
        )
        .unwrap();

    let mut init_msg = SuiteBuilder::get_default_rebalancer_register_data();

    // Test below 1 (0)
    init_msg.max_limit_bps = Some(0);

    let err = suite.register_to_rebalancer_err(0, &init_msg);
    assert!(err
        .to_string()
        .contains(&ValenceError::InvalidMaxLimitRange.to_string()));

    // test above 10000
    init_msg.max_limit_bps = Some(10001);

    // Try to register when already registered
    let err = suite.register_to_rebalancer_err(0, &init_msg);
    assert!(err
        .to_string()
        .contains(&ValenceError::InvalidMaxLimitRange.to_string()));
}

#[test]
fn test_custom_cycle_period() {
    let hour = 60 * 60;
    // the addresses are empty because they are populated in the build
    let rebalancer_init = RebalancerInstantiate::default("", "")
        .change_cycle_period(Some(hour))
        .into();
    let mut suite = SuiteBuilder::default()
        .with_custom_rebalancer(rebalancer_init)
        .build_default();

    // Do 1 rebalance
    suite.rebalance(None).unwrap();

    // Try to do another one before our cycle is passed
    suite.add_block();

    let err = suite.rebalance_err(None);
    assert_eq!(
        err,
        rebalancer::error::ContractError::CycleNotStartedYet(
            start_of_cycle(suite.app.block_info().time, hour).seconds() + hour
        )
    );

    // Pass the time to the next cycle
    suite.update_block(hour / DEFAULT_BLOCK_TIME);

    // try to do another rebalance
    suite.rebalance(None).unwrap();
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
            suite.rebalancer_addr.clone(),
            &valence_package::services::rebalancer::RebalancerExecuteMsg::ApproveAdminChange::<
                Empty,
                Empty,
            > {},
            &[],
        )
        .unwrap_err();

    suite
        .app
        .execute_contract(
            suite.admin.clone(),
            suite.rebalancer_addr.clone(),
            &valence_package::services::rebalancer::RebalancerExecuteMsg::Admin::<Empty, Empty>(
                valence_package::services::rebalancer::RebalancerAdminMsg::StartAdminChange {
                    addr: new_admin.to_string(),
                    expiration: Expiration::Never {},
                },
            ),
            &[],
        )
        .unwrap();

    suite
        .app
        .execute_contract(
            new_admin.clone(),
            suite.rebalancer_addr.clone(),
            &valence_package::services::rebalancer::RebalancerExecuteMsg::ApproveAdminChange::<
                Empty,
                Empty,
            > {},
            &[],
        )
        .unwrap();

    let admin = suite.query_admin(&suite.rebalancer_addr).unwrap();
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
            suite.rebalancer_addr.clone(),
            &valence_package::services::rebalancer::RebalancerExecuteMsg::Admin::<Empty, Empty>(
                valence_package::services::rebalancer::RebalancerAdminMsg::StartAdminChange {
                    addr: new_admin.to_string(),
                    expiration: Expiration::Never {},
                },
            ),
            &[],
        )
        .unwrap();

    suite
        .app
        .execute_contract(
            suite.admin.clone(),
            suite.rebalancer_addr.clone(),
            &valence_package::services::rebalancer::RebalancerExecuteMsg::Admin::<Empty, Empty>(
                valence_package::services::rebalancer::RebalancerAdminMsg::CancelAdminChange {},
            ),
            &[],
        )
        .unwrap();

    // Should error because we cancelled the admin change
    suite
        .app
        .execute_contract(
            new_admin,
            suite.rebalancer_addr.clone(),
            &valence_package::services::rebalancer::RebalancerExecuteMsg::ApproveAdminChange::<
                Empty,
                Empty,
            > {},
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
            suite.rebalancer_addr.clone(),
            &valence_package::services::rebalancer::RebalancerExecuteMsg::Admin::<Empty, Empty>(
                valence_package::services::rebalancer::RebalancerAdminMsg::StartAdminChange {
                    addr: new_admin.to_string(),
                    expiration: Expiration::AtHeight(suite.app.block_info().height + 5),
                },
            ),
            &[],
        )
        .unwrap();

    // Should fail because we are not the new admin
    suite
        .app
        .execute_contract(
            random_addr,
            suite.rebalancer_addr.clone(),
            &valence_package::services::rebalancer::RebalancerExecuteMsg::ApproveAdminChange::<
                Empty,
                Empty,
            > {},
            &[],
        )
        .unwrap_err();

    suite.update_block_cycle();

    // Should fail because expired
    suite
        .app
        .execute_contract(
            new_admin,
            suite.rebalancer_addr.clone(),
            &valence_package::services::rebalancer::RebalancerExecuteMsg::ApproveAdminChange::<
                Empty,
                Empty,
            > {},
            &[],
        )
        .unwrap_err();
}
