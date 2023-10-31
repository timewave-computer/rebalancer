#!/bin/bash

CHAIN=$1
shift
ORACLE_ADDR=$1
shift


if [[ "$CHAIN" == 'juno' ]]; then
  BINARY="junod"
  GAS_PRICES="0.025ujunox"
  OWNER_ADDR="juno17s47ltx2hth9w5hntncv70kvyygvg0qr83zghn"
  FEES="1000ujunox"

  ADDR_AUCTIONS_MANAGER="juno1tp2n8fa9848355hfd98lufhm84sudlvnzwvsdsqtlahtsrdtl6astvrz9j"
elif [[ "$CHAIN" == 'neutron' || "$CHAIN" == 'ntrn' ]]; then
  BINARY="neutrond"
  GAS_PRICES="0.025ntrn"
  OWNER_ADDR="neutron17s47ltx2hth9w5hntncv70kvyygvg0qr4ug32g"
  FEES="1000untrn"

  # ADDR_AUCTIONS_MANAGER=""
else
  echo "Unknown chain"
fi

# EXECUTE_FLAGS="--gas-prices $GAS_PRICES --gas auto --gas-adjustment 1.4 -y"
EXECUTE_FLAGS="--fees $FEES --gas auto --gas-adjustment 1.4 -y"

execute_msg=$(jq -n \
  --arg oracle_Addr "$ORACLE_ADDR" \
  '{admin: {
      update_oracle: {
        oracle_addr: $oracle_Addr,
      }
    }}')

$BINARY tx wasm execute $ADDR_AUCTIONS_MANAGER "$execute_msg" --from $OWNER_ADDR $EXECUTE_FLAGS