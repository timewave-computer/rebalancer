#!/bin/bash

CHAIN=$1
shift
COMMAND=$1
shift

if [[ "$CHAIN" == 'juno' ]]; then
  BINARY="junod"
  GAS_PRICES="0.025ujunox"
  OWNER_ADDR="juno17s47ltx2hth9w5hntncv70kvyygvg0qr83zghn"

  CODE_ID_ACCOUNT=3811
  CODE_ID_SERVICES_MANAGER=3815
  CODE_ID_REBALANCER=3814
  CODE_ID_ORACLE=3813
  CODE_ID_AUCTION=3810
  CODE_ID_AUCTIONS_MANAGER=3812

  # Contracts addresses for init below
  ADDR_SERVICES_MANAGER="juno1h2md5367062ypuv93kpwyu84eaq04xx4lfmqwqp5fkqrwa66pynsk6qmk5"
  ADDR_AUCTIONS_MANAGER="juno1tp2n8fa9848355hfd98lufhm84sudlvnzwvsdsqtlahtsrdtl6astvrz9j"

  # General data per chain
  WHITELISTED_DENOMS='[\"ujunox\", \"factory/juno17s47ltx2hth9w5hntncv70kvyygvg0qr83zghn/vuusdcx\"]'
  WHITELISTED_BASE_DENOMS='[\"ujunox\", \"factory/juno17s47ltx2hth9w5hntncv70kvyygvg0qr83zghn/vuusdcx\"]'
elif [[ "$CHAIN" == 'neutron' || "$CHAIN" == 'ntrn' ]]; then
  BINARY="neutrond"
  GAS_PRICES="0.075untrn"
  OWNER_ADDR="neutron1phx0sz708k3t6xdnyc98hgkyhra4tp44et5s68"

  CODE_ID_ACCOUNT=750
  CODE_ID_SERVICES_MANAGER=754
  CODE_ID_REBALANCER=755
  CODE_ID_ORACLE=753
  CODE_ID_AUCTION=751
  CODE_ID_AUCTIONS_MANAGER=752

  # Contracts addresses for init below
  ADDR_SERVICES_MANAGER="neutron1gantvpnat0la8kkkzrnj48d5d8wxdjllh5r2w4r2hcrpwy00s69quypupa"
  ADDR_AUCTIONS_MANAGER="neutron13exc5wdc7y5qpqazc34djnu934lqvfw2dru30j52ahhjep6jzx8ssjxcyz"

  # General data per chain
  WHITELISTED_DENOMS='["untrn", "ibc/C4CFF46FD6DE35CA4CF4CE031E643C8FDC9BA4B99AE598E9B0ED98FE3A2319F9"]'
  WHITELISTED_BASE_DENOMS='[{"denom": "untrn", "min_balance_limit": "10000000"}, {"denom": "ibc/C4CFF46FD6DE35CA4CF4CE031E643C8FDC9BA4B99AE598E9B0ED98FE3A2319F9", "min_balance_limit": "10000000"}]'
elif [[ "$CHAIN" == 'ntrn-testnet' ]]; then
  BINARY="neutrond"
  GAS_PRICES="0.075untrn"
  OWNER_ADDR="neutron1phx0sz708k3t6xdnyc98hgkyhra4tp44et5s68"

  CODE_ID_ACCOUNT=5677
  CODE_ID_SERVICES_MANAGER=5674
  CODE_ID_REBALANCER=5675
  CODE_ID_ORACLE=5676
  CODE_ID_AUCTION=5679
  CODE_ID_AUCTIONS_MANAGER=5673

  # Contracts addresses for init below
  ADDR_SERVICES_MANAGER="neutron13ncggwefau3xla04vlugy20meap7g7a9lf2d2sxwgwvgr9mnn3yqkpjzs6"
  ADDR_AUCTIONS_MANAGER="neutron1669ftav8rv4hjuak89w04k7f0f7m9qq9564s00ld4m8dvhsr5hfsxy3x46"

  # General data per chain
  WHITELISTED_DENOMS='["untrn", "factory/neutron1phx0sz708k3t6xdnyc98hgkyhra4tp44et5s68/rebalancer-test"]'
  WHITELISTED_BASE_DENOMS='[{"denom": "untrn", "min_balance_limit": "1000"}, {"denom": "factory/neutron1phx0sz708k3t6xdnyc98hgkyhra4tp44et5s68/rebalancer-test", "min_balance_limit": "1000"}]'
else
  echo "Unknown chain"
fi

EXECUTE_FLAGS="--gas-prices $GAS_PRICES --gas auto --gas-adjustment 1.4 --output json -y"

################################################
################### Account ####################
################################################
if [[ "$COMMAND" == 'account' ]]; then
  if [ -z "$ADDR_SERVICES_MANAGER" ]; then echo "[ERROR] Services manager address is missing for $CHAIN" && exit 1; fi

  init_msg=$(jq -n \
    --arg services_manager "$ADDR_SERVICES_MANAGER" \
    '{
      services_manager: $services_manager
    }')

  $BINARY tx wasm init $CODE_ID_ACCOUNT "$init_msg" --label "Valence account" \
    --admin $OWNER_ADDR --from $OWNER_ADDR $EXECUTE_FLAGS

################################################
############### Services Manager ###############
################################################
elif [[ "$COMMAND" == 'services-manager' ]]; then
  init_msg=$(jq -n \
    --argjson account_code_id $CODE_ID_ACCOUNT \
    '{"whitelisted_code_ids": [$account_code_id]}')

  $BINARY tx wasm init $CODE_ID_SERVICES_MANAGER "$init_msg" --label "Valence services manager" \
    --admin $OWNER_ADDR --from $OWNER_ADDR $EXECUTE_FLAGS
################################################
############### Auctions Manager ###############
################################################
elif [[ "$COMMAND" == 'auctions-manager' ]]; then
  init_msg=$(jq -n \
    --argjson auction_code_id $CODE_ID_AUCTION \
    --arg server_addr $OWNER_ADDR \
    '{ auction_code_id: $auction_code_id,
       min_auction_amount: [["untrn", {"send": "1000", "start_auction": "1000"}], ["factory/neutron1phx0sz708k3t6xdnyc98hgkyhra4tp44et5s68/rebalancer-test", {"send": "1000", "start_auction": "1000"}]],
       server_addr: $server_addr
      }')

  $BINARY tx wasm init $CODE_ID_AUCTIONS_MANAGER "$init_msg" --label "Valence auctions manager" \
    --admin $OWNER_ADDR --from $OWNER_ADDR $EXECUTE_FLAGS

################################################
################## Rebalancer ##################
################################################
elif [[ "$COMMAND" == 'rebalancer' ]]; then

  if [ -z "$ADDR_SERVICES_MANAGER" ]; then echo "[ERROR] Services manager address is missing for $CHAIN" && exit 1; fi
  if [ -z "$ADDR_AUCTIONS_MANAGER" ]; then echo "[ERROR] Auctions manager address is missing for $CHAIN" && exit 1; fi

  init_msg=$(jq -n \
    --arg services_manager_addr "$ADDR_SERVICES_MANAGER" \
    --arg auctions_manager_addr "$ADDR_AUCTIONS_MANAGER" \
    --argjson whitelist_denom "$WHITELISTED_DENOMS" \
    --argjson whitelist_base_denom "$WHITELISTED_BASE_DENOMS" \
    '{services_manager_addr: $services_manager_addr,
      auctions_manager_addr: $auctions_manager_addr,
      cycle_start: "0",
      denom_whitelist: $whitelist_denom,
      base_denom_whitelist: $whitelist_base_denom,
      cycle_period: 60,
      fees: {
        denom: "factory/neutron1phx0sz708k3t6xdnyc98hgkyhra4tp44et5s68/rebalancer-test",
        register_fee: "1000",
        resume_fee: "1000"
      },
      }')

  $BINARY tx wasm init $CODE_ID_REBALANCER "$init_msg" --label "Valence rebalancer" \
    --admin $OWNER_ADDR --from $OWNER_ADDR $EXECUTE_FLAGS

################################################
#################### Oracle ####################
################################################
elif [[ "$COMMAND" == 'oracle' ]]; then
  if [ -z "$ADDR_AUCTIONS_MANAGER" ]; then echo "[ERROR] Auctions manager address is missing for $CHAIN" && exit 1; fi

  init_msg=$(
    jq -n \
      --arg auctions_manager_addr "$ADDR_AUCTIONS_MANAGER" \
      '{
        auctions_manager_addr: $auctions_manager_addr,
        seconds_allow_manual_change: 60,
        seconds_auction_prices_fresh: 360
      }'
  )

  $BINARY tx wasm init $CODE_ID_ORACLE "$init_msg" --label "Valence oracle" \
    --admin $OWNER_ADDR --from $OWNER_ADDR $EXECUTE_FLAGS
else
  echo "Unknown command"
fi
