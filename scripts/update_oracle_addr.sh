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
  GAS_PRICES="0.075untrn"
  OWNER_ADDR="neutron1phx0sz708k3t6xdnyc98hgkyhra4tp44et5s68"
  FEES="1000untrn"

  ADDR_AUCTIONS_MANAGER="neutron13exc5wdc7y5qpqazc34djnu934lqvfw2dru30j52ahhjep6jzx8ssjxcyz"
elif [[ "$CHAIN" == 'ntrn-testnet' ]]; then
  BINARY="neutrond"
  GAS_PRICES="0.075untrn"
  OWNER_ADDR="neutron1phx0sz708k3t6xdnyc98hgkyhra4tp44et5s68"

  ADDR_AUCTIONS_MANAGER="neutron1669ftav8rv4hjuak89w04k7f0f7m9qq9564s00ld4m8dvhsr5hfsxy3x46"
else
  echo "Unknown chain"
fi

EXECUTE_FLAGS="--gas-prices $GAS_PRICES --gas auto --gas-adjustment 1.4 -y"

execute_msg=$(jq -n \
  --arg oracle_addr "$ORACLE_ADDR" \
  '{admin: {
      update_oracle: {
        oracle_addr: $oracle_addr,
      }
    }}')

$BINARY tx wasm execute $ADDR_AUCTIONS_MANAGER "$execute_msg" --from $OWNER_ADDR $EXECUTE_FLAGS
