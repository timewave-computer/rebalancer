#![allow(clippy::all)]
// Test a specific config of an account with the state of the system
// This test should give us an idea of how the next rebalance would work, and give us
// a manualy way to check what trades the rebalancer is calculating
// best for debuging live data without without for a cycle

// The state of the system includes:
// - the specific account config
// - the account balances
// - all prices from oracle

// To get the data we want, we want to do those queries:
// - account config = neutrond query wasm contract-state smart neutron1qs6mzpmcw3dvg5l8nyywetcj326scszdj7v4pfk55xwshd4prqnqfwc0z2 '{"get_config": {"addr": "[ACCOUNT_ADDR]"}}'
// - account balances = neutrond q bank balances [ACCOUNT_ADDR]
// - prices = neutrond query wasm contract-state smart neutron1s8uqyh0mmh8g66s2dectf56c08y6fvusp39undp8kf4v678ededsy6tstf '"get_all_prices"'

// NOTE - we are taking the data live, so make sure to have the neutrond set up correctly with the correct version
use std::process::Command;

use cosmwasm_std::{from_json, BlockInfo};
use valence_package::services::rebalancer::RebalancerConfig;

use crate::{
    live_debugging::{
        types::{AllPricesRes, BlockRes, ConfigRes, Prices, WhitelistDenoms, WhitelistDenomsRes},
        ORACLE_ADDR, REBALANCER_ADDR,
    },
    suite::suite_builder::SuiteBuilder,
};

use super::types::Balances;

const ACCOUNT_ADDR: &str = "neutron1yd27kgnsjkeddwtuy29q9yzrnd247v6hcml6swl54dfkcwuuytqs8ha94x";
const HEIGHT: &str = "";

#[ignore = "For debugging mainnet data"]
#[test]
fn live_debugging() {
    // If we have specifig height, query by that height
    let mut height_arg = vec![];
    let mut block_height = vec![];
    if !HEIGHT.is_empty() {
        height_arg = vec!["--height", HEIGHT];
        block_height = vec![HEIGHT];
    }

    // query mainnet for the balances of the account
    let block_output = Command::new("neutrond")
        .args(["q", "block"])
        // .args(["--node", ""])
        .args(["--chain-id", "neutron-1"])
        .args(block_height.to_vec())
        .output()
        .expect("Failed getting balances");

    let block_info: BlockInfo =
        from_json::<BlockRes>(String::from_utf8_lossy(&block_output.stdout).to_string())
            .unwrap()
            .block
            .header
            .into();

    // query mainnet for the balances of the account
    let balances_output = Command::new("neutrond")
        .args(["q", "bank", "balances", ACCOUNT_ADDR])
        // .args(["--node", ""])
        .args(["--chain-id", "neutron-1"])
        .args(["--output", "json"])
        .args(height_arg.to_vec())
        .output()
        .expect("Failed getting balances");

    let balances: Balances =
        from_json(String::from_utf8_lossy(&balances_output.stdout).to_string()).unwrap();

    // Query mainnet for the config of the account
    let q_string = format!("{{\"get_config\": {{\"addr\": \"{}\"}}}}", ACCOUNT_ADDR);
    let config_output = Command::new("neutrond")
        .args(["q", "wasm", "contract-state", "smart", REBALANCER_ADDR])
        .arg(q_string)
        // .args(["--node", ""])
        .args(["--chain-id", "neutron-1"])
        .args(["--output", "json"])
        .args(height_arg.to_vec())
        .output()
        .expect("Failed getting config");

    let config: RebalancerConfig =
        from_json::<ConfigRes>(String::from_utf8_lossy(&config_output.stdout).to_string())
            .unwrap()
            .data;

    // Query mainnet for all prices from oracle
    let q_string: &str = "\"get_all_prices\"";
    let prices_output = Command::new("neutrond")
        .args(["q", "wasm", "contract-state", "smart", ORACLE_ADDR])
        .arg(q_string)
        // .args(["--node", ])
        .args(["--chain-id", "neutron-1"])
        .args(["--output", "json"])
        .args(height_arg.to_vec())
        .output()
        .expect("Failed getting all prices");

    let prices: Prices =
        from_json::<AllPricesRes>(String::from_utf8_lossy(&prices_output.stdout).to_string())
            .unwrap()
            .data;

    // Query mainnet for the whitelist denoms
    let q_string: &str = "\"get_white_lists\"";
    let whitelists_output = Command::new("neutrond")
        .args(["q", "wasm", "contract-state", "smart", REBALANCER_ADDR])
        .arg(q_string)
        // .args(["--node", ""])
        .args(["--chain-id", "neutron-1"])
        .args(["--output", "json"])
        .args(height_arg.to_vec())
        .output()
        .expect("Failed getting whitelists");

    let whitelists: WhitelistDenoms = from_json::<WhitelistDenomsRes>(
        String::from_utf8_lossy(&whitelists_output.stdout).to_string(),
    )
    .unwrap()
    .data;

    // print the data we get from mainnet for debugging
    // println!("1. Balances: {:?}", balances);
    // println!("2. Config: {:?}", config);
    // println!("3. Prices: {:?}", prices);
    // println!("whitelists: {:?}", whitelists);

    //----------------------
    // Start mocking
    //----------------------

    let mut suite = SuiteBuilder::build_live_debug(
        block_info,
        whitelists,
        prices.clone(),
        balances.balances,
        config.clone(),
    );
    let account_config = suite
        .query_rebalancer_config(suite.account_addrs[0].clone())
        .unwrap();
    let all_prices = suite.query_oracle_all_prices(None, None);

    // make sure the config is set in our mock rebalancer
    assert_eq!(account_config, config);
    // make sure prices in oracle are matching mainnet prices
    for all_price in all_prices.iter() {
        let p = prices
            .iter()
            .find(|(pair, _)| *pair == all_price.0)
            .unwrap();
        assert_eq!(all_price.1.price, p.1.price);
    }
    // assert_eq!(all_prices, prices);

    // After we confirmed everything is in place, we can do rebalance, and read the response (events should tell us the info we need)
    let res = suite.rebalance(None).unwrap();
    // println!("Rebalance response: {:?}", res);

    // print our events for debugging
    res.events.iter().for_each(|event| {
        if event.ty.contains("valence") {
            println!(
                "Type: {} | Data: {}",
                event.attributes[1].value, event.attributes[2].value
            );
        }
    });

    let new_config = suite
        .query_rebalancer_config(suite.account_addrs[0].clone())
        .unwrap();
    println!("old config: {:?}", account_config);
    println!("New config: {:?}", new_config);
}
