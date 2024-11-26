use crate::services::ValenceServices;
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Binary, CosmosMsg};
use cw_utils::Expiration;
use valence_macros::{
    valence_account_execute_msgs, valence_rebalancer_msgs, valence_service_manager_admin_msgs,
    valence_service_manager_execute_msgs,
};

/// This is base account execute msgs,
/// it implements messages to be called on the service manager
/// as well as messages to be called by services (valence_account_execute)
#[valence_account_execute_msgs]
#[valence_rebalancer_msgs]
#[cw_serde]
pub enum AccountBaseExecuteMsg {}

/// This is services manager execute msgs,
/// implements messages to be called by accounts on the services (valence_service_execute)
#[valence_service_manager_execute_msgs]
#[valence_service_manager_admin_msgs]
#[valence_rebalancer_msgs]
#[cw_serde]
pub enum ServicesManagerExecuteMsg {}

/// Admin messages for services manager
#[cw_serde]
pub enum ServicesManagerAdminMsg {
    /// Add a service to the services manager
    AddService {
        name: ValenceServices,
        addr: String,
    },
    /// Update a service name to address data
    UpdateService {
        name: ValenceServices,
        addr: String,
    },
    /// Delete service from the services manager
    RemoveService {
        name: ValenceServices,
    },
    UpdateCodeIdWhitelist {
        to_add: Vec<u64>,
        to_remove: Vec<u64>,
    },
    StartAdminChange {
        addr: String,
        expiration: Expiration,
    },
    CancelAdminChange {},
    Withdraw {
        denom: String,
    },
}
