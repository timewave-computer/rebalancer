use cosmwasm_std::{testing::mock_env, Timestamp};

use crate::suite::suite::{ATOM, NTRN, OSMO};

#[derive(Clone)]
pub struct RebalancerInstantiate {
    pub msg: rebalancer::msg::InstantiateMsg,
}

impl From<RebalancerInstantiate> for rebalancer::msg::InstantiateMsg {
    fn from(value: RebalancerInstantiate) -> Self {
        value.msg
    }
}

impl RebalancerInstantiate {
    pub fn default(services_manager: &str, auctions_manager: &str) -> Self {
        Self {
            msg: rebalancer::msg::InstantiateMsg {
                denom_whitelist: vec![ATOM.to_string(), NTRN.to_string(), OSMO.to_string()],
                base_denom_whitelist: vec![ATOM.to_string(), NTRN.to_string()],
                services_manager_addr: services_manager.to_string(),
                cycle_start: mock_env().block.time,
                auctions_manager_addr: auctions_manager.to_string(), // to modify
            },
        }
    }

    pub fn new(
        denom_whitelist: Vec<String>,
        base_denom_whitelist: Vec<String>,
        cycle_start: Timestamp,
        services_manager: &str,
        auctions_manager: &str,
    ) -> Self {
        Self {
            msg: rebalancer::msg::InstantiateMsg {
                denom_whitelist,
                base_denom_whitelist,
                services_manager_addr: services_manager.to_string(), // to modify
                cycle_start,
                auctions_manager_addr: auctions_manager.to_string(), // to modify
            },
        }
    }

    /* Change functions */
    pub fn change_denom_whitelist(&mut self, denom_whitelist: Vec<String>) {
        self.msg.denom_whitelist = denom_whitelist;
    }

    pub fn change_base_denom_whitelist(&mut self, base_denom_whitelist: Vec<String>) {
        self.msg.base_denom_whitelist = base_denom_whitelist;
    }

    pub fn change_service_manager(&mut self, services_manager: &str) {
        self.msg.services_manager_addr = services_manager.to_string();
    }

    pub fn change_cycle_start(&mut self, cycle_start: Timestamp) {
        self.msg.cycle_start = cycle_start;
    }

    pub fn change_auctions_manager(&mut self, auctions_manager: &str) {
        self.msg.auctions_manager_addr = auctions_manager.to_string();
    }
}