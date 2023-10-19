use cosmwasm_std::Addr;
use cw_storage_plus::Item;

/// State to store the address of the services manager contract.
pub const SERVICES_MANAGER: Item<Addr> = Item::new("services_manager");

/// State to store the address of the admin of the contract.
pub const ADMIN: Item<Addr> = Item::new("admin");