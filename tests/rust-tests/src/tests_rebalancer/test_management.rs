use std::collections::HashSet;

use cosmwasm_std::{
    coin, coins, testing::mock_env, to_json_binary, Addr, OverflowError, StdError, Timestamp,
    Uint128,
};
use cw_multi_test::Executor;
use valence_package::services::{
    rebalancer::{
        BaseDenom, PauseReason, RebalancerUpdateData, ServiceFeeConfig, SystemRebalanceStatus,
    },
    ValenceServices,
};

use crate::suite::{
    contracts::account_contract,
    suite::{Suite, ATOM, NTRN, TRUSTEE},
    suite_builder::SuiteBuilder,
};

#[test]
fn test_remove_trustee() {
    let mut data = SuiteBuilder::get_default_rebalancer_register_data();
    data.trustee = Some(TRUSTEE.to_string());
    let mut suite = SuiteBuilder::default()
        .with_rebalancer_data(vec![data])
        .build_default();

    let config = suite
        .query_rebalancer_config(suite.account_addrs[0].clone())
        .unwrap();
    assert_eq!(config.trustee, Some(Addr::unchecked(TRUSTEE)));

    suite
        .update_config(
            suite.owner.clone(),
            0,
            ValenceServices::Rebalancer,
            RebalancerUpdateData {
                trustee: Some(valence_package::helpers::OptionalField::Clear),
                base_denom: None,
                targets: HashSet::new(),
                pid: None,
                max_limit_bps: None,
                target_override_strategy: None,
            },
        )
        .unwrap();

    let config = suite
        .query_rebalancer_config(suite.account_addrs[0].clone())
        .unwrap();
    assert_eq!(config.trustee, None);
}

#[test]
fn test_not_whitelisted_base_denom() {
    let not_whitelisted_base_denom = "not_whitelisted_base_denom".to_string();
    let mut suite = SuiteBuilder::default().build_default();

    let config = suite
        .query_rebalancer_config(suite.account_addrs[0].clone())
        .unwrap();
    assert_eq!(
        config.base_denom,
        SuiteBuilder::get_default_rebalancer_register_data().base_denom
    );

    let err: rebalancer::error::ContractError = suite
        .update_config(
            suite.owner.clone(),
            0,
            ValenceServices::Rebalancer,
            RebalancerUpdateData {
                trustee: None,
                base_denom: Some(not_whitelisted_base_denom.clone()),
                targets: HashSet::new(),
                pid: None,
                max_limit_bps: None,
                target_override_strategy: None,
            },
        )
        .unwrap_err()
        .downcast()
        .unwrap();

    assert_eq!(
        err,
        rebalancer::error::ContractError::BaseDenomNotWhitelisted(not_whitelisted_base_denom)
    )
}

#[test]
fn test_multiple_min_balance_on_update() {
    let mut data = SuiteBuilder::get_default_rebalancer_register_data();

    // set min_balance to both targets
    let mut targets = SuiteBuilder::get_default_targets();
    targets[0].min_balance = Some(100_u128.into());
    targets[1].min_balance = Some(100_u128.into());

    data.targets = HashSet::from_iter(targets.iter().cloned());

    let mut suite = SuiteBuilder::default().build_default();

    let err: rebalancer::error::ContractError = suite
        .update_config(
            suite.owner.clone(),
            0,
            ValenceServices::Rebalancer,
            RebalancerUpdateData {
                trustee: None,
                base_denom: None,
                targets: data.targets,
                pid: None,
                max_limit_bps: None,
                target_override_strategy: None,
            },
        )
        .unwrap_err()
        .downcast()
        .unwrap();

    assert_eq!(
        err,
        rebalancer::error::ContractError::MultipleMinBalanceTargets
    )
}

#[test]
fn test_not_whitelisted_denom_on_update() {
    let not_whitelisted_denom = "not_whitelisted_denom".to_string();
    let mut data = SuiteBuilder::get_default_rebalancer_register_data();

    // set min_balance to both targets
    let mut targets = SuiteBuilder::get_default_targets();
    targets[0].denom = not_whitelisted_denom.to_string();

    data.targets = HashSet::from_iter(targets.iter().cloned());

    let mut suite = SuiteBuilder::default().build_default();

    let err: rebalancer::error::ContractError = suite
        .update_config(
            suite.owner.clone(),
            0,
            ValenceServices::Rebalancer,
            RebalancerUpdateData {
                trustee: None,
                base_denom: None,
                targets: data.targets,
                pid: None,
                max_limit_bps: None,
                target_override_strategy: None,
            },
        )
        .unwrap_err()
        .downcast()
        .unwrap();

    assert_eq!(
        err,
        rebalancer::error::ContractError::DenomNotWhitelisted(not_whitelisted_denom)
    )
}

#[test]
fn test_invalid_targets_perc_on_update() {
    let mut data = SuiteBuilder::get_default_rebalancer_register_data();

    // set min_balance to both targets
    let mut targets = SuiteBuilder::get_default_targets();
    targets[0].bps = 5000;
    targets[1].bps = 6000;

    data.targets = HashSet::from_iter(targets.iter().cloned());

    let mut suite = SuiteBuilder::default().build_default();

    let err: rebalancer::error::ContractError = suite
        .update_config(
            suite.owner.clone(),
            0,
            ValenceServices::Rebalancer,
            RebalancerUpdateData {
                trustee: None,
                base_denom: None,
                targets: data.targets,
                pid: None,
                max_limit_bps: None,
                target_override_strategy: None,
            },
        )
        .unwrap_err()
        .downcast()
        .unwrap();

    assert_eq!(
        err,
        rebalancer::error::ContractError::InvalidTargetPercentage(1.1.to_string(),)
    )
}

#[test]
fn test_not_admin_admin() {
    let mut suite = SuiteBuilder::default().build_default();

    suite.update_rebalancer_system_status_err(
        Addr::unchecked("not_admin"),
        SystemRebalanceStatus::NotStarted {
            cycle_start: Timestamp::from_nanos(0),
        },
    );
}

#[test]
fn test_update_status() {
    let mut suite = SuiteBuilder::default().build_default();

    let status = suite.query_rebalancer_system_status().unwrap();
    assert_eq!(
        status,
        SystemRebalanceStatus::NotStarted {
            cycle_start: mock_env().block.time
        }
    );

    suite
        .update_rebalancer_system_status(
            suite.admin.clone(),
            SystemRebalanceStatus::Finished {
                next_cycle: Timestamp::from_nanos(0),
            },
        )
        .unwrap();

    let status = suite.query_rebalancer_system_status().unwrap();
    assert_eq!(
        status,
        SystemRebalanceStatus::Finished {
            next_cycle: Timestamp::from_nanos(0)
        }
    );

    // try update status to processing (Should error)
    let err = suite.update_rebalancer_system_status_err(
        suite.admin.clone(),
        SystemRebalanceStatus::Processing {
            cycle_started: Timestamp::from_nanos(0),
            start_from: Addr::unchecked("random"),
            prices: vec![],
        },
    );

    assert_eq!(
        err,
        rebalancer::error::ContractError::CantUpdateStatusToProcessing
    );
}

#[test]
fn test_update_whitelist() {
    let mut suite = SuiteBuilder::default().build_default();

    let whitelist = suite.query_rebalancer_whitelists().unwrap();

    // lets make sure the whitelist is what we expect for the tests
    assert!(whitelist.denom_whitelist.contains(ATOM));
    assert!(whitelist.denom_whitelist.len() == 3);
    assert!(whitelist
        .base_denom_whitelist
        .iter()
        .any(|bd| bd.denom == *ATOM));
    assert!(whitelist.base_denom_whitelist.len() == 2);

    // remove atom, add random
    let to_add: Vec<String> = vec!["random".to_string()];
    let to_remove = vec![ATOM.to_string(), NTRN.to_string()];

    suite
        .update_rebalancer_denom_whitelist(suite.admin.clone(), to_add, to_remove)
        .unwrap();

    let whitelist = suite.query_rebalancer_whitelists().unwrap();
    assert!(!whitelist.denom_whitelist.contains(ATOM));
    assert!(!whitelist.denom_whitelist.contains(NTRN));
    assert!(whitelist.denom_whitelist.len() == 2);

    // remove atom, add random
    let to_add: Vec<BaseDenom> = vec![BaseDenom {
        denom: "random".to_string(),
        min_balance_limit: Uint128::one(),
    }];
    let to_remove = vec![ATOM.to_string(), NTRN.to_string()];

    suite
        .update_rebalancer_base_denom_whitelist(suite.admin.clone(), to_add, to_remove)
        .unwrap();

    let whitelist = suite.query_rebalancer_whitelists().unwrap();
    assert!(!whitelist
        .base_denom_whitelist
        .iter()
        .any(|bd| bd.denom == *ATOM));
    assert!(!whitelist
        .base_denom_whitelist
        .iter()
        .any(|bd| bd.denom == *NTRN));
    assert!(whitelist.base_denom_whitelist.len() == 1);
    assert!(whitelist
        .base_denom_whitelist
        .iter()
        .any(|bd| bd.denom == *"random"));
}

#[test]
fn test_update_addrs() {
    let mut suite = SuiteBuilder::default().build_default();

    // make sure addresses are correct first
    let addrs = suite.query_rebalancer_managers().unwrap();

    assert_eq!(addrs.services, suite.manager_addr);
    assert_eq!(addrs.auctions, suite.auctions_manager_addr);

    let random_addr = Addr::unchecked("random");
    suite
        .update_rebalancer_services_manager_address(suite.admin.clone(), random_addr.clone())
        .unwrap();
    suite
        .update_rebalancer_auctions_manager_address(suite.admin.clone(), random_addr.clone())
        .unwrap();

    let addrs = suite.query_rebalancer_managers().unwrap();

    assert_eq!(addrs.services, random_addr);
    assert_eq!(addrs.auctions, random_addr);
}

#[test]
fn test_register_wrong_code_id() {
    let mut suite = Suite::default();

    // Try to register using a not allowed code id
    let err: services_manager::error::ContractError = suite
        .app
        .execute_contract(
            suite.rebalancer_addr.clone(),
            suite.manager_addr.clone(),
            &valence_package::msgs::core_execute::ServicesManagerExecuteMsg::RegisterToService {
                service_name: ValenceServices::Rebalancer,
                data: Some(
                    to_json_binary(&SuiteBuilder::get_default_rebalancer_register_data()).unwrap(),
                ),
            },
            &[],
        )
        .unwrap_err()
        .downcast()
        .unwrap();

    assert_eq!(
        err,
        services_manager::error::ContractError::NotWhitelistedContract(3)
    );

    // Update code id whitelist
    suite.app.execute_contract(
        suite.admin.clone(),
        suite.manager_addr.clone(),
        &valence_package::msgs::core_execute::ServicesManagerExecuteMsg::Admin(
            valence_package::msgs::core_execute::ServicesManagerAdminMsg::UpdateCodeIdWhitelist {
                to_add: vec![3],
                to_remove: vec![],
            },
        ),
        &[],
    ).unwrap();

    // Send tokens to rebalancer to have minimum balance
    suite
        .app
        .send_tokens(
            suite.admin.clone(),
            suite.rebalancer_addr.clone(),
            &coins(100, ATOM.to_string()),
        )
        .unwrap();

    // try to register again using the same contract as above
    suite
        .app
        .execute_contract(
            suite.rebalancer_addr.clone(),
            suite.manager_addr.clone(),
            &valence_package::msgs::core_execute::ServicesManagerExecuteMsg::RegisterToService {
                service_name: ValenceServices::Rebalancer,
                data: Some(
                    to_json_binary(&SuiteBuilder::get_default_rebalancer_register_data()).unwrap(),
                ),
            },
            &[],
        )
        .unwrap();
}

#[test]
fn test_update_config_not_whitelsited_denom() {
    let mut suite = Suite::default();

    suite
        .update_rebalancer_denom_whitelist(suite.admin.clone(), vec![], vec![NTRN.to_string()])
        .unwrap();

    let err: rebalancer::error::ContractError = suite
        .update_config(
            suite.owner.clone(),
            0,
            ValenceServices::Rebalancer,
            RebalancerUpdateData {
                trustee: Some(valence_package::helpers::OptionalField::Clear),
                base_denom: None,
                targets: HashSet::new(),
                pid: None,
                max_limit_bps: None,
                target_override_strategy: None,
            },
        )
        .unwrap_err()
        .downcast()
        .unwrap();

    assert_eq!(
        err,
        rebalancer::error::ContractError::DenomNotWhitelisted(NTRN.to_string())
    )
}

#[test]
fn test_account_balance_limit() {
    let mut suite = Suite::default();
    let (account_position_1, _) = suite.create_temp_account(&[]);
    let (account_position_2, account_addr_2) =
        suite.create_temp_account(&coins(1000, ATOM.to_string()));
    let (account_position_3, account_addr_3) =
        suite.create_temp_account(&coins(1000, NTRN.to_string()));
    let register_data = SuiteBuilder::get_default_rebalancer_register_data();

    // Register 2 and 3 as normal because they have enough balance
    suite
        .register_to_rebalancer(account_position_2, &register_data)
        .unwrap();
    suite
        .register_to_rebalancer(account_position_3, &register_data)
        .unwrap();

    // Registering should fail, as the temp account doesn't have any tokens
    let err = suite.register_to_rebalancer_err(account_position_1, &register_data);
    assert_eq!(
        err,
        rebalancer::error::ContractError::InvalidAccountMinValue(
            Uint128::zero().to_string(),
            Uint128::from(100_u128).to_string()
        )
    );

    // Remove funds from account 2 to 0 (mimic balance is 0)
    suite
        .app
        .send_tokens(
            account_addr_2.clone(),
            suite.admin.clone(),
            &coins(1000, ATOM.to_string()),
        )
        .unwrap();

    // Remove funds from account 3 to 50 (mimic balance is above 0 and below minimum)
    suite
        .app
        .send_tokens(
            account_addr_3.clone(),
            suite.admin.clone(),
            &coins(950, NTRN.to_string()),
        )
        .unwrap();

    // make sure account 2 and 3 are not paused
    suite
        .query_rebalancer_config(account_addr_2.clone())
        .unwrap();

    suite
        .query_rebalancer_config(account_addr_3.clone())
        .unwrap();

    // do a rebalance with an account with no balance
    suite.rebalance_with_update_block(None).unwrap();

    // Account 2 and 3 should be paused now.
    suite
        .query_rebalancer_paused_config(account_addr_2.clone())
        .unwrap();

    suite
        .query_rebalancer_paused_config(account_addr_3.clone())
        .unwrap();

    // Try to resume without enough balance
    let err = suite.resume_service_err(account_position_2, ValenceServices::Rebalancer);
    assert_eq!(
        err,
        rebalancer::error::ContractError::InvalidAccountMinValue(
            Uint128::zero().to_string(),
            Uint128::from(100_u128).to_string()
        )
    );

    let err = suite.resume_service_err(account_position_3, ValenceServices::Rebalancer);
    assert_eq!(
        err,
        rebalancer::error::ContractError::InvalidAccountMinValue(
            Uint128::from(33_u128).to_string(),
            Uint128::from(100_u128).to_string()
        )
    );

    // Send enough tokens to resume successfully
    suite
        .app
        .send_tokens(
            suite.admin.clone(),
            account_addr_2,
            &coins(100, ATOM.to_string()),
        )
        .unwrap();

    suite
        .app
        .send_tokens(
            suite.admin.clone(),
            account_addr_3,
            &coins(300, NTRN.to_string()),
        )
        .unwrap();

    // Resume should work now
    suite
        .resume_service(account_position_2, ValenceServices::Rebalancer)
        .unwrap();
    suite
        .resume_service(account_position_3, ValenceServices::Rebalancer)
        .unwrap();
}

#[test]
fn test_with_fee() {
    let mut suite = Suite::default();

    suite
        .update_rebalancer_fees(ServiceFeeConfig {
            denom: NTRN.to_string(),
            register_fee: 100_u128.into(),
            resume_fee: 100_u128.into(),
        })
        .unwrap();

    let (account_position, _) = suite.create_temp_account(&coins(1000, ATOM.to_string()));
    let register_data = SuiteBuilder::get_default_rebalancer_register_data();

    // Register account without enough fee token should fail.
    let err = suite.register_to_rebalancer_fee_err(account_position, &register_data);
    assert_eq!(
        err,
        StdError::overflow(OverflowError::new(
            cosmwasm_std::OverflowOperation::Sub,
            "0",
            "100"
        ))
    );

    // set balance of account to ntrn
    suite.set_balance(account_position, coin(1000, NTRN.to_string()));

    // Should successfully register
    suite
        .register_to_rebalancer(account_position, &register_data)
        .unwrap();

    // Account balance should be - 900 ntrn
    let balance = suite
        .app
        .wrap()
        .query_balance(suite.get_account_addr(account_position), NTRN.to_string())
        .unwrap();
    assert!(balance.amount == Uint128::new(900_u128))
}

#[test]
fn test_manual_resume_without_fee() {
    let mut suite = Suite::default();

    suite
        .update_rebalancer_fees(ServiceFeeConfig {
            denom: NTRN.to_string(),
            register_fee: 100_u128.into(),
            resume_fee: 100_u128.into(),
        })
        .unwrap();

    let (account_position, _) = suite.create_temp_account(&coins(1000, NTRN.to_string()));
    let register_data = SuiteBuilder::get_default_rebalancer_register_data();
    suite
        .register_to_rebalancer(account_position, &register_data)
        .unwrap();

    let balance = suite
        .app
        .wrap()
        .query_balance(suite.get_account_addr(account_position), NTRN.to_string())
        .unwrap();
    assert_eq!(balance.amount, Uint128::new(900_u128));

    // account pause the rebalancer
    suite
        .pause_service(account_position, ValenceServices::Rebalancer)
        .unwrap();

    let paused_config = suite
        .query_rebalancer_paused_config(suite.get_account_addr(account_position))
        .unwrap();
    assert_eq!(
        paused_config.reason,
        PauseReason::AccountReason("Some reason".to_string())
    );

    suite
        .resume_service(account_position, ValenceServices::Rebalancer)
        .unwrap();

    suite
        .query_rebalancer_config(suite.get_account_addr(account_position))
        .unwrap();

    // Verify fee was not taken from the account, (still 900 NTRN)
    let balance = suite
        .app
        .wrap()
        .query_balance(suite.get_account_addr(account_position), NTRN.to_string())
        .unwrap();
    assert_eq!(balance.amount, Uint128::new(900_u128));
}

#[test]
fn test_resume_with_fee() {
    let mut suite = Suite::default();

    suite
        .update_rebalancer_fees(ServiceFeeConfig {
            denom: NTRN.to_string(),
            register_fee: 100_u128.into(),
            resume_fee: 100_u128.into(),
        })
        .unwrap();

    let (account_position, _) = suite.create_temp_account(&coins(1000, NTRN.to_string()));
    let register_data = SuiteBuilder::get_default_rebalancer_register_data();
    suite
        .register_to_rebalancer(account_position, &register_data)
        .unwrap();

    suite.set_balance(account_position, coin(0, NTRN.to_string()));

    suite.rebalance(None).unwrap();

    // Account should be paused because of empty balance
    let paused_config = suite
        .query_rebalancer_paused_config(suite.get_account_addr(account_position))
        .unwrap();
    assert_eq!(paused_config.reason, PauseReason::EmptyBalance);

    suite.set_balance(account_position, coin(1000, NTRN.to_string()));

    suite
        .resume_service(account_position, ValenceServices::Rebalancer)
        .unwrap();

    // Verify the account is resumed (if we get config, it means it's not paused)
    suite
        .query_rebalancer_config(suite.get_account_addr(account_position))
        .unwrap();

    // Verify balance is 900 because we paid 100 for resume fee
    let balance = suite
        .app
        .wrap()
        .query_balance(suite.get_account_addr(account_position), NTRN.to_string())
        .unwrap();
    assert_eq!(balance.amount, Uint128::new(900_u128));
}

#[test]
fn test_fee_withdraw() {
    let mut suite = Suite::default();

    suite
        .update_rebalancer_fees(ServiceFeeConfig {
            denom: NTRN.to_string(),
            register_fee: 100_u128.into(),
            resume_fee: 100_u128.into(),
        })
        .unwrap();

    let (account_position, _) = suite.create_temp_account(&coins(1000, NTRN.to_string()));
    let register_data = SuiteBuilder::get_default_rebalancer_register_data();

    // Register account without enough fee token should fail.
    suite
        .register_to_rebalancer(account_position, &register_data)
        .unwrap();

    // account balance should be 900 (1000 - 100)
    let balance = suite
        .app
        .wrap()
        .query_balance(suite.get_account_addr(account_position), NTRN.to_string())
        .unwrap();
    assert_eq!(balance.amount, Uint128::new(900_u128));

    // manager should have the 100 NTRN fee
    let manager_balance = suite
        .app
        .wrap()
        .query_balance(suite.manager_addr.clone(), NTRN.to_string())
        .unwrap();
    assert_eq!(manager_balance.amount, Uint128::new(100_u128));

    // Get initial admin balance of NTRN
    let admin_initial_balance = suite
        .app
        .wrap()
        .query_balance(suite.admin.clone(), NTRN.to_string())
        .unwrap();

    // Do the withdraw from naanger (should send it to the admin)
    suite.withdraw_fees_from_manager(NTRN).unwrap();

    // Get balance of the admin after withdraw, should be extra 100 NTRN
    let admin_new_balance = suite
        .app
        .wrap()
        .query_balance(suite.admin.clone(), NTRN.to_string())
        .unwrap();

    assert_eq!(
        admin_new_balance.amount,
        admin_initial_balance.amount + Uint128::new(100_u128)
    );
}

#[test]
fn test_not_whitelisted_account_after_register() {
    let mut suite = Suite::default();

    let account_addr = suite.account_addrs[0].clone();

    let non_whitelisted_account_code_id = suite.app.store_code(account_contract());

    // migrate the account to rebalancer code id (not a whitelisted account code id)
    suite
        .app
        .migrate_contract(
            suite.owner.clone(),
            account_addr.clone(),
            &valence_account::msg::MigrateMsg::NoStateChange {},
            non_whitelisted_account_code_id,
        )
        .unwrap();

    // Try to update config when the account code id is not whitelisted
    let mock_rebalancer_data = SuiteBuilder::get_default_rebalancer_register_data();

    let err = suite.update_config_err(
        suite.owner.clone(),
        0,
        ValenceServices::Rebalancer,
        mock_rebalancer_data,
    );

    assert_eq!(
        err,
        services_manager::error::ContractError::NotWhitelistedContract(
            non_whitelisted_account_code_id
        )
    );

    suite.rebalance(None).unwrap();

    // Make sure the account is paused with the correct issue
    let paused_config = suite.query_rebalancer_paused_config(account_addr).unwrap();
    assert_eq!(
        paused_config.reason,
        PauseReason::NotWhitelistedAccountCodeId(non_whitelisted_account_code_id)
    );

    // try to resume the account
    let err = suite
        .resume_service(0, ValenceServices::Rebalancer)
        .unwrap_err()
        .downcast::<services_manager::error::ContractError>()
        .unwrap();
    assert_eq!(
        err,
        services_manager::error::ContractError::NotWhitelistedContract(
            non_whitelisted_account_code_id
        )
    );
}
