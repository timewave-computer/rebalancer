use std::borrow::BorrowMut;

use auction_package::Pair;
use price_oracle::state::PriceStep;

use crate::suite::{
    suite::{ATOM, NTRN, OSMO},
    suite_builder::SuiteBuilder,
};

#[test]
fn test_add_path_for_pair() {
    let mut suite = SuiteBuilder::default().build_basic(true);

    // denom1 is the 2nd denom, so its wrong and should error
    let path = vec![PriceStep {
        denom1: suite.pair.1.to_string(),
        denom2: suite.pair.0.to_string(),
        pool_address: suite
            .astro_pools
            .get(&suite.pair.clone().into())
            .unwrap()
            .clone(),
    }];
    let err = suite.add_astro_path_to_oracle_err(suite.pair.clone(), path);
    assert_eq!(err, price_oracle::error::ContractError::PricePathIsWrong);

    // Should be successful
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
}

// ATOM-NTRN is stable, so the price should be 1-ish
#[test]
fn test_basic_astro_default() {
    let mut suite = SuiteBuilder::default().build_basic(true);

    let old_oracle_last_local_price = suite.query_oracle_local_price(suite.pair.clone())[0].clone();
    // This should error because we don't have astro path for the price yet
    // and no auction ran so far
    let err = suite.update_price_err(suite.pair.clone());
    assert_eq!(
        err,
        price_oracle::error::ContractError::NoAstroPath(suite.pair.clone())
    );

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

    // Make sure the pool price is not the same as the old price
    // To confirm the price actually changed later
    assert_ne!(pool_price, old_oracle_last_local_price.price);

    // Try to update again
    suite.update_price(suite.pair.clone()).unwrap();

    // Verify we do get an acceptable price (query price from pool)
    let oracle_last_local_price = suite.query_oracle_local_price(suite.pair.clone())[0].clone();
    println!("pool_price: {:?}", pool_price);
    println!("oracle_price: {:?}", oracle_last_local_price);
    assert_eq!(oracle_last_local_price.price, pool_price);
}

// Test for a pair that doesn't have a direct pool, so the astro path is 2 pools
#[test]
fn test_complex_astro_default() {
    let mut suite = SuiteBuilder::default().build_basic(true);
    let complex_pair = Pair::from((ATOM.to_string(), OSMO.to_string()));

    let old_oracle_price = suite.query_oracle_price(complex_pair.clone());
    // Register the astro path
    let path = vec![
        PriceStep {
            denom1: ATOM.to_string(),
            denom2: NTRN.to_string(),
            pool_address: suite
                .astro_pools
                .get(&suite.pair.clone().into())
                .unwrap()
                .clone(),
        },
        PriceStep {
            denom1: NTRN.to_string(),
            denom2: OSMO.to_string(),
            pool_address: suite
                .astro_pools
                .get(&(NTRN.to_string(), OSMO.to_string()))
                .unwrap()
                .clone(),
        },
    ];
    suite
        .add_astro_path_to_oracle(complex_pair.clone(), path)
        .unwrap();

    // Randomize the pool a little to get a "nice" price
    let mut rng = rand::thread_rng();

    for _ in 0..100 {
        suite.do_random_swap(
            rng.borrow_mut(),
            Pair::from((ATOM.to_string(), NTRN.to_string())),
            100_000_u128,
            1_000_000_u128,
        );

        suite.do_random_swap(
            rng.borrow_mut(),
            Pair::from((NTRN.to_string(), OSMO.to_string())),
            100_000_u128,
            1_000_000_u128,
        );
    }

    // Try to update again
    suite.update_price(complex_pair.clone()).unwrap();

    let oracle_price = suite.query_oracle_price(complex_pair);
    // let pool_atom_ntrn_price = suite.query_astro_pool_price(
    //     suite
    //         .astro_pools
    //         .get(&suite.pair.clone().into())
    //         .unwrap()
    //         .clone(),
    //     suite.pair.clone(),
    // );
    // let pool_ntrn_osmo_price = suite.query_astro_pool_price(
    //     suite
    //         .astro_pools
    //         .get(&(NTRN.to_string(), OSMO.to_string()))
    //         .unwrap()
    //         .clone(),
    //     (NTRN.to_string(), OSMO.to_string()).into(),
    // );

    println!("old_oracle_price: {:?}", old_oracle_price);
    println!("oracle_price: {:?}", oracle_price);
    // println!("pool_atom_ntrn_price: {:?}", pool_atom_ntrn_price);
    // println!("pool_ntrn_osmo_price: {:?}", pool_ntrn_osmo_price);

    // make sure the old price is not the same as new price
    assert_ne!(oracle_price.price, old_oracle_price.price);
}
