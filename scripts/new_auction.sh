#!/bin/bash

CHAIN=$1
shift

if [[ "$CHAIN" == 'juno' ]]; then
  BINARY="junod"
  GAS_PRICES="0.025ujunox"
  OWNER_ADDR="juno17s47ltx2hth9w5hntncv70kvyygvg0qr83zghn"

  ADDR_AUCTIONS_MANAGER="juno1arszzw6yytxtq2l07eaqhuhradnmkdwftwc6vp3j3xaxgnlg3scq2fe4cn"
elif [[ "$CHAIN" == 'neutron' || "$CHAIN" == 'ntrn' ]]; then
  BINARY="neutrond"
  GAS_PRICES="0.025ntrn"
  OWNER_ADDR="neutron17s47ltx2hth9w5hntncv70kvyygvg0qr4ug32g"

  # ADDR_AUCTIONS_MANAGER=""
else
  echo "Unknown chain"
fi

EXECUTE_FLAGS="--gas-prices $GAS_PRICES --gas auto --gas-adjustment 1.4 -y"

## You can change value manually and uncomment it here
PAIR='["ibc/C4CFF46FD6DE35CA4CF4CE031E643C8FDC9BA4B99AE598E9B0ED98FE3A2319F9", "ujunox"]'
AUCTION_STRATEGY='{ "start_price_perc": 2000, "end_price_perc": 2000 }'
CHAIN_HALT='{ "cap": "14400", "block_avg": "3" }'
PRICE_FRESHNESS='{ "limit": "3", "multipliers": [["2", "2"], ["1", "1.5"]] }'

while [[ "$#" -gt 0 ]]; do
  case $1 in
  -p | --pair)
    PAIR="$2"
    shift
    ;;
  -as | --auction-strategy)
    AUCTION_STRATEGY="$2"
    shift
    ;;
  -ch | --chain-halt)
    CHAIN_HALT="$2"
    shift
    ;;
  -pf | --price-freshness)
    PRICE_FRESHNESS="$2"
    shift
    ;;
  *)
    echo "Unknown parameter passed: $1"
    exit 1
    ;;
  esac
  shift
done

execute_msg=$(jq -n \
  --argjson pair "$PAIR" \
  --argjson auction_strategy "$AUCTION_STRATEGY" \
  --argjson chain_halt_config "$CHAIN_HALT" \
  --argjson price_freshness_strategy "$PRICE_FRESHNESS" \
  '{admin: {
      new_auction: {
        msg: {
          pair: $pair,
          auction_strategy: $auction_strategy,
          chain_halt_config: $chain_halt_config,
          price_freshness_strategy: $price_freshness_strategy
        },
      }
    }}')

$BINARY tx wasm execute $ADDR_AUCTIONS_MANAGER "$execute_msg" --from $OWNER_ADDR $EXECUTE_FLAGS
