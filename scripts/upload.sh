#!/bin/bash

CHAIN=$1
shift
COMMAND=$1
shift
INIT_BY=$1
shift

if [[ "$CHAIN" == 'juno' ]]; then
  BINARY="junod"
  GAS_PRICES="0.025ujunox"
  OWNER_ADDR="juno17s47ltx2hth9w5hntncv70kvyygvg0qr83zghn"

elif [[ "$CHAIN" == 'neutron' || "$CHAIN" == 'ntrn' ]]; then
  BINARY="neutrond"
  GAS_PRICES="0.075untrn"
  OWNER_ADDR="neutron1phx0sz708k3t6xdnyc98hgkyhra4tp44et5s68"
  AUCTIONS_MANAGER_ADDR="neutron13exc5wdc7y5qpqazc34djnu934lqvfw2dru30j52ahhjep6jzx8ssjxcyz"

elif [[ "$CHAIN" == 'ntrn-testnet' ]]; then
  BINARY="neutrond"
  GAS_PRICES="0.075untrn"
  OWNER_ADDR="neutron1phx0sz708k3t6xdnyc98hgkyhra4tp44et5s68"
  AUCTIONS_MANAGER_ADDR="neutron1669ftav8rv4hjuak89w04k7f0f7m9qq9564s00ld4m8dvhsr5hfsxy3x46"
  
else

  echo "Unknown chain"
fi

if [ -z "$INIT_BY" ]; then
  ADDRESSES="$OWNER_ADDR,$AUCTIONS_MANAGER_ADDR"
else
  ADDRESSES="$OWNER_ADDR,$AUCTIONS_MANAGER_ADDR,$INIT_BY"
fi

EXECUTE_FLAGS="--gas-prices $GAS_PRICES --gas auto --gas-adjustment 1.4 --output json --instantiate-anyof-addresses $ADDRESSES -y"
ACCOUNT_EXECUTE_FLAGS="--gas-prices $GAS_PRICES --gas auto --gas-adjustment 1.4 --output json -y"
ARTIFACTS_PATH="../artifacts"

# File names
ACCOUNT_FILE_NAME="$ARTIFACTS_PATH/valence_account.wasm"
AUCTION_FILE_NAME="$ARTIFACTS_PATH/auction.wasm"
AUCTIONS_MANAGER_FILE_NAME="$ARTIFACTS_PATH/auctions_manager.wasm"
ORACLE_FILE_NAME="$ARTIFACTS_PATH/price_oracle.wasm"
SERVICES_MANAGER_FILE_NAME="$ARTIFACTS_PATH/services_manager.wasm"
REBALANCER_FILE_NAME="$ARTIFACTS_PATH/rebalancer.wasm"

if [[ "$COMMAND" == 'account' ]]; then
  $BINARY tx wasm s $ACCOUNT_FILE_NAME --from $OWNER_ADDR $ACCOUNT_EXECUTE_FLAGS
elif [[ "$COMMAND" == 'auction' ]]; then
  $BINARY tx wasm s $AUCTION_FILE_NAME --from $OWNER_ADDR $EXECUTE_FLAGS
elif [[ "$COMMAND" == 'auctions-manager' ]]; then
  $BINARY tx wasm s $AUCTIONS_MANAGER_FILE_NAME --from $OWNER_ADDR $EXECUTE_FLAGS
elif [[ "$COMMAND" == 'oracle' ]]; then
  $BINARY tx wasm s $ORACLE_FILE_NAME --from $OWNER_ADDR $EXECUTE_FLAGS
elif [[ "$COMMAND" == 'services-manager' ]]; then
  $BINARY tx wasm s $SERVICES_MANAGER_FILE_NAME --from $OWNER_ADDR $EXECUTE_FLAGS
elif [[ "$COMMAND" == 'rebalancer' ]]; then
  $BINARY tx wasm s $REBALANCER_FILE_NAME --from $OWNER_ADDR $EXECUTE_FLAGS
else
  echo "Unknown command"
fi
