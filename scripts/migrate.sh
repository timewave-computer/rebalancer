#!/bin/bash

COMMAND=$1
shift

BINARY="neutrond"
GAS_PRICES="0.075untrn"
EXECUTE_FLAGS="--gas-prices $GAS_PRICES --gas auto --gas-adjustment 1.4 --output json -y"

# addresses
ADMIN_ADDR="neutron1phx0sz708k3t6xdnyc98hgkyhra4tp44et5s68"
REBALANCER_ADDR="neutron1qs6mzpmcw3dvg5l8nyywetcj326scszdj7v4pfk55xwshd4prqnqfwc0z2"
ORACLE_ADDR="neutron1s8uqyh0mmh8g66s2dectf56c08y6fvusp39undp8kf4v678ededsy6tstf"
SERVICES_MANAGER_ADDR="neutron1gantvpnat0la8kkkzrnj48d5d8wxdjllh5r2w4r2hcrpwy00s69quypupa"
AUCTIONS_MANAGER_ADDR="neutron13exc5wdc7y5qpqazc34djnu934lqvfw2dru30j52ahhjep6jzx8ssjxcyz"

# Code ids
SERVICES_MANAGER_CODE_ID=1615
REBALANCER_CODE_ID=1616
AUCTIONS_MANAGER_CODE_ID=1614
AUCTION_CODE_ID=1617
ORACLE_CODE_ID=1619
ACCOUNT_CODE_ID=1618

# array of pairs we have to migrate
declare -a PAIRS=(
    '["ibc/C4CFF46FD6DE35CA4CF4CE031E643C8FDC9BA4B99AE598E9B0ED98FE3A2319F9","untrn"]'
    '["untrn","ibc/C4CFF46FD6DE35CA4CF4CE031E643C8FDC9BA4B99AE598E9B0ED98FE3A2319F9"]'
    '["ibc/B559A80D62249C8AA07A380E2A2BEA6E5CA9A6F079C912C3A9E9B494105E4F81","ibc/C4CFF46FD6DE35CA4CF4CE031E643C8FDC9BA4B99AE598E9B0ED98FE3A2319F9"]'
    '["ibc/B559A80D62249C8AA07A380E2A2BEA6E5CA9A6F079C912C3A9E9B494105E4F81","untrn"]'
    '["ibc/C4CFF46FD6DE35CA4CF4CE031E643C8FDC9BA4B99AE598E9B0ED98FE3A2319F9","ibc/B559A80D62249C8AA07A380E2A2BEA6E5CA9A6F079C912C3A9E9B494105E4F81"]'
    '["untrn","ibc/B559A80D62249C8AA07A380E2A2BEA6E5CA9A6F079C912C3A9E9B494105E4F81"]'
    '["factory/neutron1p8d89wvxyjcnawmgw72klknr3lg9gwwl6ypxda/newt","ibc/C4CFF46FD6DE35CA4CF4CE031E643C8FDC9BA4B99AE598E9B0ED98FE3A2319F9"]'
    '["factory/neutron1p8d89wvxyjcnawmgw72klknr3lg9gwwl6ypxda/newt","untrn"]'
    '["factory/neutron1p8d89wvxyjcnawmgw72klknr3lg9gwwl6ypxda/newt","ibc/B559A80D62249C8AA07A380E2A2BEA6E5CA9A6F079C912C3A9E9B494105E4F81"]'
    '["ibc/C4CFF46FD6DE35CA4CF4CE031E643C8FDC9BA4B99AE598E9B0ED98FE3A2319F9","factory/neutron1p8d89wvxyjcnawmgw72klknr3lg9gwwl6ypxda/newt"]'
    '["untrn","factory/neutron1p8d89wvxyjcnawmgw72klknr3lg9gwwl6ypxda/newt"]'
    '["ibc/B559A80D62249C8AA07A380E2A2BEA6E5CA9A6F079C912C3A9E9B494105E4F81","factory/neutron1p8d89wvxyjcnawmgw72klknr3lg9gwwl6ypxda/newt"]'
)

# default migrate message where there is no state change
NO_STATE_MIGRATE_MSG=$(
    jq -n \
        '{no_state_change: {}}'
)

# migrate each contract using a differetn command (arg)
if [[ "$COMMAND" == 'services-manager' ]]; then

    $BINARY tx wasm migrate $SERVICES_MANAGER_ADDR $SERVICES_MANAGER_CODE_ID "$NO_STATE_MIGRATE_MSG" --from $ADMIN_ADDR $EXECUTE_FLAGS

elif [[ "$COMMAND" == 'rebalancer' ]]; then

    $BINARY tx wasm migrate $REBALANCER_ADDR $REBALANCER_CODE_ID "$NO_STATE_MIGRATE_MSG" --from $ADMIN_ADDR $EXECUTE_FLAGS

elif [[ "$COMMAND" == 'auctions-manager' ]]; then

    $BINARY tx wasm migrate $AUCTIONS_MANAGER_ADDR $AUCTIONS_MANAGER_CODE_ID "$NO_STATE_MIGRATE_MSG" --from $ADMIN_ADDR $EXECUTE_FLAGS

elif [[ "$COMMAND" == 'oracle' ]]; then

    $BINARY tx wasm migrate $ORACLE_ADDR $ORACLE_CODE_ID "$NO_STATE_MIGRATE_MSG" --from $ADMIN_ADDR $EXECUTE_FLAGS

elif [[ "$COMMAND" == 'all-auctions' ]]; then

    for pair in "${PAIRS[@]}"; do
        echo "Migrating pair: $pair"

        EXECUTE_MSG=$(
            jq -n \
                --argjson pair $pair \
                --argjson code_id $AUCTION_CODE_ID \
                '{admin:{
                    migrate_auction: {
                        pair: $pair, 
                        code_id: $code_id, 
                        msg: {"no_state_change": {}}
                    }
                }}'
        )

        $BINARY tx wasm execute $AUCTIONS_MANAGER_ADDR "$EXECUTE_MSG" --from $ADMIN_ADDR $EXECUTE_FLAGS
        sleep 8
    done

elif [[ "$COMMAND" == 'code-id-updates' ]]; then
    # We need to update our contracts to include the new code ids we are using

    EXECUTE_MSG=$(
        jq -n \
            --argjson code_id $AUCTION_CODE_ID \
            '{admin: {
                    update_auction_id:{
                        code_id: $code_id
                    }
                }}'
    )

    $BINARY tx wasm execute $AUCTIONS_MANAGER_ADDR "$EXECUTE_MSG" --from $ADMIN_ADDR $EXECUTE_FLAGS
    sleep 8

    EXECUTE_MSG=$(
        jq -n \
            --argjson code_id $ACCOUNT_CODE_ID \
            '{admin: {
                update_code_id_whitelist: {
                    to_add: [$code_id], 
                    to_remove: []
                }
            }}'
    )
    $BINARY tx wasm execute $SERVICES_MANAGER_ADDR "$EXECUTE_MSG" --from $ADMIN_ADDR $EXECUTE_FLAGS
    
else
    echo "Unknown command"
fi
