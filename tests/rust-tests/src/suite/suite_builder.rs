use std::{
    borrow::BorrowMut,
    collections::{HashMap, HashSet},
};

use auction_package::{states::MinAmount, Pair};
use cosmwasm_schema::serde;
use cosmwasm_std::{coin, coins, from_json, Addr, Decimal, Uint128};
use cw_multi_test::{App, AppBuilder, Executor};
use cw_storage_plus::Item;
use valence_package::services::{
    rebalancer::{Target, TargetOverrideStrategy, PID},
    ValenceServices,
};

use super::{
    contracts::{
        account_contract, astro_coin_registry_contract, astro_factory_contract,
        astro_pair_contract, astro_pair_stable_contract, astro_token_contract, auction_contract,
        auctions_manager_contract, oracle_contract, rebalancer_contract, services_manager_contract,
    },
    instantiates::{
        AccountInstantiate, AuctionInstantiate, AuctionsManagerInstantiate, OracleInstantiate,
        RebalancerInstantiate, ServicesManagerInstantiate,
    },
    suite::{
        Suite, ACC_OWNER, ADMIN, ATOM, DEFAULT_BALANCE_AMOUNT, DEFAULT_D, DEFAULT_I,
        DEFAULT_NTRN_PRICE_BPS, DEFAULT_OSMO_PRICE_BPS, DEFAULT_P, MM, NTRN, OSMO, TRUSTEE,
    },
};

pub(crate) struct SuiteBuilder {
    // Users
    pub admin: Addr,
    pub owner: Addr,
    pub trustee: Addr,
    pub mm: Addr,

    // Account data
    pub account_num: u64,
    pub rebalancer_register_datas: Vec<valence_package::services::rebalancer::RebalancerData>,

    // Code ids of contracts
    pub account_code_id: u64,
    pub manager_code_id: u64,
    pub rebalancer_code_id: u64,
    pub auction_code_id: u64,
    pub auctions_manager_code_id: u64,
    pub oracle_code_id: u64,

    // for custom init of contracts
    pub custom_rebalancer_init: Option<rebalancer::msg::InstantiateMsg>,

    // Flags for adding stuff to builds for tests
    pub add_oracle_addr: bool,

    // astro code id
    pub astro_factory_code_id: u64,
    pub astro_registery_code_id: u64,
    pub astro_token_code_id: u64,
    pub astro_pair_code_id: u64,
    pub astro_stable_pair_code_id: u64,
}

impl Default for SuiteBuilder {
    fn default() -> Self {
        let admin = Addr::unchecked(ADMIN);
        let owner = Addr::unchecked(ACC_OWNER);
        let trustee = Addr::unchecked(TRUSTEE);
        let mm = Addr::unchecked(MM);

        Self {
            account_num: 1,
            rebalancer_register_datas: vec![SuiteBuilder::get_default_rebalancer_register_data()],
            admin,
            owner,
            trustee,
            mm,
            account_code_id: 100000,
            manager_code_id: 100000,
            rebalancer_code_id: 100000,
            auction_code_id: 100000,
            auctions_manager_code_id: 100000,
            oracle_code_id: 100000,
            custom_rebalancer_init: None,
            add_oracle_addr: true,
            astro_factory_code_id: 100000,
            astro_registery_code_id: 100000,
            astro_token_code_id: 100000,
            astro_pair_code_id: 100000,
            astro_stable_pair_code_id: 100000,
        }
    }
}

// get defaults
impl SuiteBuilder {
    pub fn get_default_targets() -> Vec<Target> {
        vec![
            Target {
                denom: ATOM.to_string(),
                bps: 7500,
                // min_balance: Some(7800_u128.into()),
                min_balance: None,
            },
            Target {
                denom: NTRN.to_string(),
                bps: 2500,
                min_balance: None,
            },
        ]
    }

    pub fn get_default_rebalancer_register_data(
    ) -> valence_package::services::rebalancer::RebalancerData {
        let mut targets = HashSet::with_capacity(2);

        targets.insert(Target {
            denom: ATOM.to_string(),
            bps: 7500,
            // min_balance: Some(7800_u128.into()),
            min_balance: None,
        });
        targets.insert(Target {
            denom: NTRN.to_string(),
            bps: 2500,
            min_balance: None,
        });

        valence_package::services::rebalancer::RebalancerData {
            trustee: None,
            base_denom: ATOM.to_string(),
            targets,
            pid: PID {
                p: DEFAULT_P.to_string(),
                i: DEFAULT_I.to_string(),
                d: DEFAULT_D.to_string(),
            },
            max_limit_bps: None,
            target_override_strategy: TargetOverrideStrategy::Proportional,
            account_type: Default::default(),
        }
    }

    pub fn get_rebalancer_register_data_with_trustee(
    ) -> valence_package::services::rebalancer::RebalancerData {
        let mut data = SuiteBuilder::get_default_rebalancer_register_data();
        data.trustee = Some(TRUSTEE.to_string());
        data
    }
}

// Helpers to modify the build process
impl SuiteBuilder {
    pub fn with_accounts(&mut self, accounts_num: u64) -> &mut Self {
        self.account_num = accounts_num;
        self
    }

    pub fn with_rebalancer_data(
        &mut self,
        data: Vec<valence_package::services::rebalancer::RebalancerData>,
    ) -> &mut Self {
        self.rebalancer_register_datas = data;
        self
    }

    pub fn with_custom_rebalancer(&mut self, data: rebalancer::msg::InstantiateMsg) -> &mut Self {
        self.custom_rebalancer_init = Some(data);
        self
    }

    pub fn without_oracle_addr(&mut self) -> &mut Self {
        self.add_oracle_addr = false;
        self
    }
}

// Modular build process
impl SuiteBuilder {
    /// Upload all the contracts we use in our testing
    pub fn upload_contracts(&mut self, app: &mut App) {
        self.account_code_id = app.store_code(account_contract());
        self.manager_code_id = app.store_code(services_manager_contract());
        self.rebalancer_code_id = app.store_code(rebalancer_contract());
        self.auction_code_id = app.store_code(auction_contract());
        self.auctions_manager_code_id = app.store_code(auctions_manager_contract());
        self.oracle_code_id = app.store_code(oracle_contract());
        self.astro_factory_code_id = app.store_code(astro_factory_contract());
        self.astro_registery_code_id = app.store_code(astro_coin_registry_contract());
        self.astro_token_code_id = app.store_code(astro_token_contract());
        self.astro_pair_code_id = app.store_code(astro_pair_contract());
        self.astro_stable_pair_code_id = app.store_code(astro_pair_stable_contract());
    }

    pub fn init_auctions_manager(
        &mut self,
        app: &mut App,
        init_msg: auctions_manager::msg::InstantiateMsg,
    ) -> Addr {
        app.instantiate_contract(
            self.auctions_manager_code_id,
            self.admin.clone(),
            &init_msg,
            &[],
            "auctions_manager",
            Some(self.admin.to_string()),
        )
        .unwrap()
    }

    pub fn init_oracle(
        &mut self,
        app: &mut App,
        init_msg: price_oracle::msg::InstantiateMsg,
    ) -> Addr {
        app.instantiate_contract(
            self.oracle_code_id,
            self.admin.clone(),
            &init_msg,
            &[],
            "oracle",
            Some(self.admin.to_string()),
        )
        .unwrap()
    }

    fn init_auctions(
        &mut self,
        app: &mut App,
        with_prices: bool,
    ) -> (Addr, Addr, HashMap<(String, String), Addr>) {
        // init Auctions manager contract
        let auctions_manager_addr = self.init_auctions_manager(
            app,
            AuctionsManagerInstantiate::new(self.auction_code_id, self.mm.to_string()).into(),
        );

        // init price_oracle contract
        let price_oracle_addr = self.init_oracle(
            app,
            OracleInstantiate::default(auctions_manager_addr.clone()).into(),
        );

        // If we don't want to add it to the manager
        if self.add_oracle_addr {
            // Update the oracle addr on the manager
            app.execute_contract(
                self.admin.clone(),
                auctions_manager_addr.clone(),
                &auctions_manager::msg::ExecuteMsg::Admin(Box::new(
                    auctions_manager::msg::AdminMsgs::UpdateOracle {
                        oracle_addr: price_oracle_addr.to_string(),
                    },
                )),
                &[],
            )
            .unwrap();
        }

        // init auction for each pair
        // atom-ntrn
        app.execute_contract(
            self.admin.clone(),
            auctions_manager_addr.clone(),
            &auctions_manager::msg::ExecuteMsg::Admin(Box::new(
                auctions_manager::msg::AdminMsgs::NewAuction {
                    msg: AuctionInstantiate::atom_ntrn().into(),
                    label: "atom-ntrn".to_string(),
                    min_amount: Some(MinAmount {
                        send: Uint128::new(5),
                        start_auction: Uint128::new(10),
                    }),
                },
            )),
            &[],
        )
        .unwrap();

        // atom-osmo
        app.execute_contract(
            self.admin.clone(),
            auctions_manager_addr.clone(),
            &auctions_manager::msg::ExecuteMsg::Admin(Box::new(
                auctions_manager::msg::AdminMsgs::NewAuction {
                    msg: AuctionInstantiate::atom_osmo().into(),
                    label: "atom-osmo".to_string(),
                    min_amount: Some(MinAmount {
                        send: Uint128::new(5),
                        start_auction: Uint128::new(10),
                    }),
                },
            )),
            &[],
        )
        .unwrap();

        // ntrn-atom
        app.execute_contract(
            self.admin.clone(),
            auctions_manager_addr.clone(),
            &auctions_manager::msg::ExecuteMsg::Admin(Box::new(
                auctions_manager::msg::AdminMsgs::NewAuction {
                    msg: AuctionInstantiate::ntrn_atom().into(),
                    label: "ntrn-atom".to_string(),
                    min_amount: Some(MinAmount {
                        send: Uint128::new(5),
                        start_auction: Uint128::new(10),
                    }),
                },
            )),
            &[],
        )
        .unwrap();

        // ntrn-osmo
        app.execute_contract(
            self.admin.clone(),
            auctions_manager_addr.clone(),
            &auctions_manager::msg::ExecuteMsg::Admin(Box::new(
                auctions_manager::msg::AdminMsgs::NewAuction {
                    msg: AuctionInstantiate::ntrn_osmo().into(),
                    label: "ntrn-osmo".to_string(),
                    min_amount: Some(MinAmount {
                        send: Uint128::new(5),
                        start_auction: Uint128::new(10),
                    }),
                },
            )),
            &[],
        )
        .unwrap();

        // osmo-atom
        app.execute_contract(
            self.admin.clone(),
            auctions_manager_addr.clone(),
            &auctions_manager::msg::ExecuteMsg::Admin(Box::new(
                auctions_manager::msg::AdminMsgs::NewAuction {
                    msg: AuctionInstantiate::osmo_atom().into(),
                    label: "osmo-atom".to_string(),
                    min_amount: Some(MinAmount {
                        send: Uint128::new(5),
                        start_auction: Uint128::new(10),
                    }),
                },
            )),
            &[],
        )
        .unwrap();

        // osmo-ntrn
        app.execute_contract(
            self.admin.clone(),
            auctions_manager_addr.clone(),
            &auctions_manager::msg::ExecuteMsg::Admin(Box::new(
                auctions_manager::msg::AdminMsgs::NewAuction {
                    msg: AuctionInstantiate::osmo_ntrn().into(),
                    label: "osmo_ntrn".to_string(),
                    min_amount: Some(MinAmount {
                        send: Uint128::new(5),
                        start_auction: Uint128::new(10),
                    }),
                },
            )),
            &[],
        )
        .unwrap();

        // Update price for pairs
        let pairs = vec![
            (
                Pair::from((ATOM.to_string(), NTRN.to_string())),
                Decimal::bps(DEFAULT_NTRN_PRICE_BPS),
            ),
            (
                Pair::from((ATOM.to_string(), OSMO.to_string())),
                Decimal::bps(DEFAULT_OSMO_PRICE_BPS),
            ),
            (
                Pair::from((NTRN.to_string(), ATOM.to_string())),
                Decimal::one() / Decimal::bps(DEFAULT_NTRN_PRICE_BPS),
            ),
            (
                Pair::from((OSMO.to_string(), ATOM.to_string())),
                Decimal::one() / Decimal::bps(DEFAULT_OSMO_PRICE_BPS),
            ),
            (
                Pair::from((NTRN.to_string(), OSMO.to_string())),
                Decimal::bps(DEFAULT_OSMO_PRICE_BPS) / Decimal::bps(DEFAULT_NTRN_PRICE_BPS),
            ),
            (
                Pair::from((OSMO.to_string(), NTRN.to_string())),
                Decimal::bps(DEFAULT_NTRN_PRICE_BPS) / Decimal::bps(DEFAULT_OSMO_PRICE_BPS),
            ),
        ];
        let mut auctions = HashMap::<(String, String), Addr>::new();

        for (pair, price) in pairs {
            if with_prices {
                // update price
                app.execute_contract(
                    self.admin.clone(),
                    price_oracle_addr.clone(),
                    &price_oracle::msg::ExecuteMsg::ManualPriceUpdate {
                        pair: pair.clone(),
                        price,
                    },
                    &[],
                )
                .unwrap();
            }

            let auction_addr: Addr = app
                .wrap()
                .query_wasm_smart(
                    auctions_manager_addr.clone(),
                    &auction_package::msgs::AuctionsManagerQueryMsg::GetPairAddr {
                        pair: pair.clone(),
                    },
                )
                .unwrap();
            auctions.insert(pair.into(), auction_addr);
        }

        (auctions_manager_addr, price_oracle_addr, auctions)
    }

    pub fn init_manager(&mut self, app: &mut App) -> Addr {
        let services_manager_init_msg: services_manager::msg::InstantiateMsg =
            ServicesManagerInstantiate::new(vec![self.account_code_id]).into();

        app.instantiate_contract(
            self.manager_code_id,
            self.admin.clone(),
            &services_manager_init_msg,
            &[],
            "services_manager",
            Some(self.admin.to_string()),
        )
        .unwrap()
    }

    pub fn init_rebalancer(
        &mut self,
        app: &mut App,
        auctions_manager_addr: Addr,
        manager_addr: Addr,
    ) -> Addr {
        let rebalancer_instantiate_msg: rebalancer::msg::InstantiateMsg =
            if let Some(mut custom_rebalancer_init) = self.custom_rebalancer_init.clone() {
                custom_rebalancer_init.auctions_manager_addr = auctions_manager_addr.to_string();
                custom_rebalancer_init.services_manager_addr = manager_addr.to_string();
                custom_rebalancer_init
            } else {
                RebalancerInstantiate::default(
                    manager_addr.as_str(),
                    auctions_manager_addr.as_str(),
                )
                .into()
            };

        app.instantiate_contract(
            self.rebalancer_code_id,
            self.admin.clone(),
            &rebalancer_instantiate_msg,
            &[],
            "rebalancer",
            Some(self.admin.to_string()),
        )
        .unwrap()
    }

    pub fn init_accounts(&mut self, app: &mut App, manager_addr: Addr) -> Vec<Addr> {
        let account_init_msg: valence_account::msg::InstantiateMsg =
            AccountInstantiate::new(manager_addr.as_str()).into();
        let mut accounts: Vec<Addr> = vec![];

        for x in 0..self.account_num {
            // Instantiate the account contract
            let account_addr = app
                .borrow_mut()
                .instantiate_contract(
                    self.account_code_id,
                    self.owner.clone(),
                    &account_init_msg,
                    &coins(1000, ATOM.to_string()),
                    format!("account_{x}"),
                    Some(self.owner.to_string()),
                )
                .unwrap();
            accounts.push(account_addr.clone());
        }

        accounts
    }
}

// Build functions
impl SuiteBuilder {
    pub fn set_app(&mut self) -> App {
        let balances = vec![
            coin(DEFAULT_BALANCE_AMOUNT.u128(), ATOM.to_string()),
            coin(DEFAULT_BALANCE_AMOUNT.u128(), NTRN.to_string()),
            coin(DEFAULT_BALANCE_AMOUNT.u128(), OSMO.to_string()),
        ];

        AppBuilder::new().build(|router, _, storage| {
            // Give admin the balances
            router
                .bank
                .init_balance(storage, &self.admin, balances.clone())
                .unwrap();

            // give owner the balances to funds his accounts
            router
                .bank
                .init_balance(storage, &self.owner, balances.clone())
                .unwrap();

            router
                .bank
                .init_balance(storage, &self.mm, balances)
                .unwrap();
        })
    }
    /// build a basic suite that upload all contracts and init contracts
    pub fn build_basic(&mut self, with_price: bool) -> Suite {
        let mut app = self.set_app();

        // upload contracts
        self.upload_contracts(app.borrow_mut());

        // Init auction
        let (auctions_manager_addr, oracle_addr, auction_addrs) =
            self.init_auctions(app.borrow_mut(), with_price);

        // Init services manager
        let manager_addr = self.init_manager(app.borrow_mut());

        // Init rebalancer contract
        let rebalancer_addr = self.init_rebalancer(
            app.borrow_mut(),
            auctions_manager_addr.clone(),
            manager_addr.clone(),
        );

        // Init accounts based on the amount is set
        let account_addrs = self.init_accounts(app.borrow_mut(), manager_addr.clone());

        let pools = self.init_astro(app.borrow_mut());

        Suite {
            app,
            admin: self.admin.clone(),
            owner: self.owner.clone(),
            trustee: self.trustee.clone(),
            mm: self.mm.clone(),
            oracle_addr,
            auctions_manager_addr,
            manager_addr,
            rebalancer_addr,
            account_addrs,
            auction_addrs,
            account_code_id: self.account_code_id,
            pair: Pair::from((ATOM.to_string(), NTRN.to_string())),
            astro_pools: pools,
        }
    }

    /// Does a basic build but also add service to manager and register accounts to the services
    pub fn build_default(&mut self) -> Suite {
        let mut suite = self.build_basic(true);

        // Add the rebalancer to the services manager
        suite
            .add_service_to_manager(
                self.admin.clone(),
                suite.manager_addr.clone(),
                ValenceServices::Rebalancer,
                suite.rebalancer_addr.to_string(),
            )
            .unwrap();

        for account_position in 0..suite.account_addrs.len() {
            if account_position < self.rebalancer_register_datas.len() {
                suite
                    .register_to_service(
                        suite.owner.clone(),
                        account_position as u64,
                        ValenceServices::Rebalancer,
                        self.rebalancer_register_datas[account_position].clone(),
                    )
                    .unwrap();
            } else {
                suite
                    .register_to_service(
                        suite.owner.clone(),
                        account_position as u64,
                        ValenceServices::Rebalancer,
                        SuiteBuilder::get_default_rebalancer_register_data().clone(),
                    )
                    .unwrap();
            }
            // Register the account to the rebalancer
        }

        suite
    }
}

// Queries
impl SuiteBuilder {
    pub fn query_wasm_raw_item<T: for<'de> serde::de::Deserialize<'de> + serde::ser::Serialize>(
        app: &App,
        contract_addr: Addr,
        item: Item<T>,
    ) -> T {
        let res: Vec<u8> = app
            .wrap()
            .query_wasm_raw(contract_addr, item.as_slice())
            .unwrap()
            .unwrap();
        from_json::<T>(&res).unwrap()
    }
}
