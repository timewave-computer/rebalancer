use auction::msg::NewAuctionParams;
use auction_package::{
    helpers::ChainHaltConfig, states::MinAmount, AuctionStrategy, Pair, PriceFreshnessStrategy,
};
use cosmwasm_schema::cw_serde;
use cw_utils::Expiration;

#[cw_serde]
pub struct InstantiateMsg {
    pub auction_code_id: u64,
    pub min_auction_amount: Vec<(String, MinAmount)>,
    pub server_addr: String,
}

#[cw_serde]
pub enum ExecuteMsg {
    AuctionFunds { pair: Pair },
    WithdrawFunds { pair: Pair },
    FinishAuction { pair: Pair, limit: u64 },
    ApproveAdminChange {},
    Admin(Box<AdminMsgs>),
    Server(ServerMsgs),
}

#[cw_serde]
pub enum MigrateMsg {
    NoStateChange {},
    ToV1 {},
}

#[cw_serde]
pub enum ServerMsgs {
    OpenAuction {
        pair: Pair,
        params: NewAuctionParams,
    },
}

#[cw_serde]
pub enum AdminMsgs {
    NewAuction {
        msg: auction::msg::InstantiateMsg,
        label: String,
        min_amount: Option<MinAmount>,
    },
    PauseAuction {
        pair: Pair,
    },
    ResumeAuction {
        pair: Pair,
    },
    UpdateAuctionId {
        code_id: u64,
    },
    UpdateOracle {
        oracle_addr: String,
    },
    UpdateMinAmount {
        denom: String,
        min_amount: MinAmount,
    },
    UpdateStrategy {
        pair: Pair,
        strategy: AuctionStrategy,
    },
    UpdateChainHaltConfig {
        pair: Pair,
        halt_config: ChainHaltConfig,
    },
    UpdatePriceFreshnessStrategy {
        pair: Pair,
        strategy: PriceFreshnessStrategy,
    },
    MigrateAuction {
        pair: Pair,
        code_id: u64,
        msg: auction::msg::MigrateMsg,
    },
    ChangeServerAddr {
        addr: String,
    },
    StartAdminChange {
        addr: String,
        expiration: Expiration,
    },
    CancelAdminChange {},
}
