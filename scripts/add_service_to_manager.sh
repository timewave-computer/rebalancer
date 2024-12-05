#!/bin/bash

CHAIN=$1
shift
SERVICE_NAME=$1
shift
SERVICE_ADDR=$1
shift

if [[ "$CHAIN" == 'juno' ]]; then
  BINARY="junod"
  GAS_PRICES="0.025ujunox"
  OWNER_ADDR="juno17s47ltx2hth9w5hntncv70kvyygvg0qr83zghn"
  FEES="1000ujunox"

  ADDR_SERVICES_MANAGER="juno1wh5gyyd3hhaeq6jgnawcecvgear7k8c94celuqqxcrz65sglemlql37ple"
elif [[ "$CHAIN" == 'neutron' || "$CHAIN" == 'ntrn' ]]; then
  BINARY="neutrond"
  GAS_PRICES="0.075untrn"
  OWNER_ADDR="neutron1phx0sz708k3t6xdnyc98hgkyhra4tp44et5s68"
  FEES="1000untrn"

  ADDR_SERVICES_MANAGER="neutron1gantvpnat0la8kkkzrnj48d5d8wxdjllh5r2w4r2hcrpwy00s69quypupa"
elif [[ "$CHAIN" == 'ntrn-testnet' ]]; then
  BINARY="neutrond"
  GAS_PRICES="0.075untrn"
  OWNER_ADDR="neutron1phx0sz708k3t6xdnyc98hgkyhra4tp44et5s68"
  FEES="1000untrn"

  ADDR_SERVICES_MANAGER="neutron13ncggwefau3xla04vlugy20meap7g7a9lf2d2sxwgwvgr9mnn3yqkpjzs6"
else
  echo "Unknown chain"
fi

EXECUTE_FLAGS="--gas-prices $GAS_PRICES --gas auto --gas-adjustment 1.4 -y"
# EXECUTE_FLAGS="--fees $FEES --gas auto --gas-adjustment 1.4 -y"

execute_msg=$(jq -n \
  --arg service_name "$SERVICE_NAME" \
  --arg service_addr "$SERVICE_ADDR" \
  '{admin: {
      add_service: {
        name: $service_name,
        addr: $service_addr
      }
    }}')

$BINARY tx wasm execute $ADDR_SERVICES_MANAGER "$execute_msg" --from $OWNER_ADDR $EXECUTE_FLAGS
