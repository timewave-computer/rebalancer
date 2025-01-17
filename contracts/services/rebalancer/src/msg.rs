use std::collections::HashSet;

use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Coin, Timestamp};
use valence_macros::valence_service_query_msgs;
use valence_package::{
    services::rebalancer::{
        BaseDenom, PauseData, RebalancerConfig, ServiceFeeConfig, SystemRebalanceStatus,
    },
    states::QueryFeeAction,
};

#[cw_serde]
pub struct InstantiateMsg {
    pub denom_whitelist: Vec<String>,
    pub base_denom_whitelist: Vec<BaseDenom>,
    pub services_manager_addr: String,
    pub cycle_start: Timestamp,
    pub auctions_manager_addr: String,
    pub cycle_period: Option<u64>,
    pub fees: ServiceFeeConfig,
}

#[valence_service_query_msgs]
#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(RebalancerConfig)]
    GetConfig { addr: String },
    #[returns(Vec<(Addr, RebalancerConfig)>)]
    GetAllConfigs {
        start_after: Option<String>,
        limit: Option<u32>,
    },
    #[returns(PauseData)]
    GetPausedConfig { addr: String },
    #[returns(SystemRebalanceStatus)]
    GetSystemStatus,
    #[returns(WhitelistsResponse)]
    GetWhiteLists,
    #[returns(ManagersAddrsResponse)]
    GetManagersAddrs,
    #[returns(Addr)]
    GetAdmin,
}

#[cw_serde]
pub enum MigrateMsg {
    NoStateChange {},
}

#[cw_serde]
pub struct WhitelistsResponse {
    pub denom_whitelist: HashSet<String>,
    pub base_denom_whitelist: HashSet<BaseDenom>,
}

#[cw_serde]
pub struct ManagersAddrsResponse {
    pub services: Addr,
    pub auctions: Addr,
}
