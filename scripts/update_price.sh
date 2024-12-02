#!/bin/bash

CHAIN=$1
shift
PAIR1=$1
shift
PAIR2=$1
shift
PRICE=$1
shift

if [[ "$CHAIN" == 'juno' ]]; then
  BINARY="junod"
  GAS_PRICES="0.025ujunox"
  OWNER_ADDR="juno17s47ltx2hth9w5hntncv70kvyygvg0qr83zghn"
  FEES="10000ujunox"

  ORACLE_ADDR="juno14vgs85az6xlfzkczzq06agk2tv8zkdxqdue4gs08h0f60smu3jjqfryaj2"
elif [[ "$CHAIN" == 'neutron' || "$CHAIN" == 'ntrn' ]]; then
  BINARY="neutrond"
  GAS_PRICES="0.075untrn"
  OWNER_ADDR="neutron1phx0sz708k3t6xdnyc98hgkyhra4tp44et5s68"
  FEES="1000untrn"

  ORACLE_ADDR="neutron1s8uqyh0mmh8g66s2dectf56c08y6fvusp39undp8kf4v678ededsy6tstf"
elif [[ "$CHAIN" == 'ntrn-testnet' ]]; then
  BINARY="neutrond"
  GAS_PRICES="0.075untrn"
  OWNER_ADDR="neutron1phx0sz708k3t6xdnyc98hgkyhra4tp44et5s68"
  FEES="1000untrn"

  ORACLE_ADDR="neutron1g4qcmk65nw57hmqlzk6cejnftg20zmctky0l2epdfz3npw3x2cmqprul6f"
else
  echo "Unknown chain"
fi

EXECUTE_FLAGS="--gas-prices $GAS_PRICES --gas auto --gas-adjustment 1.4 --output json -y"
# EXECUTE_FLAGS="--fees $FEES --gas auto --gas-adjustment 1.4 -y"

if [ -z "$ORACLE_ADDR" ]; then echo "[ERROR] Oracle address is missing for $CHAIN" && exit 1; fi

if [ -z "$PRICE" ]; then
  execute_msg=$(jq -n \
    --arg pair1 "$PAIR1" \
    --arg pair2 "$PAIR2" \
    '{update_price: {
      pair: [$pair1, $pair2],
    }}')

else
  execute_msg=$(jq -n \
    --arg pair1 "$PAIR1" \
    --arg pair2 "$PAIR2" \
    --arg price "$PRICE" \
    '{manual_price_update: {
      pair: [$pair1, $pair2],
      price: $price,
    }}')

fi

$BINARY tx wasm execute $ORACLE_ADDR "$execute_msg" --from $OWNER_ADDR $EXECUTE_FLAGS
