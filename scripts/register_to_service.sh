ACCOUNT_ADDR=$1
shift

execute_msg=$(jq -n \
  '{
  "register_to_service": {
    "service_name": "rebalancer",
    "data": "ewoiYmFzZV9kZW5vbSI6ICJmYWN0b3J5L25ldXRyb24xcGh4MHN6NzA4azN0NnhkbnljOThoZ2t5aHJhNHRwNDRldDVzNjgvcmViYWxhbmNlci10ZXN0IiwKInBpZCI6IHsgInAiOiAiMC4xIiwgImkiOiAiMCIsICJkIjogIjAiIH0sCiJ0YXJnZXRfb3ZlcnJpZGVfc3RyYXRlZ3kiOiAicHJvcG9ydGlvbmFsIiwKInRhcmdldHMiOiBbCnsKImRlbm9tIjogImZhY3RvcnkvbmV1dHJvbjFwaHgwc3o3MDhrM3Q2eGRueWM5OGhna3locmE0dHA0NGV0NXM2OC9yZWJhbGFuY2VyLXRlc3QiLAoiYnBzIjogNTAwMAp9LAp7ICJkZW5vbSI6ICJ1bnRybiIsICJicHMiOiA1MDAwIH0KXQp9"
  }
}')

EXECUTE_FLAGS="--gas-prices 0.075untrn --gas auto --gas-adjustment 1.4 --output json -y"

neutrond tx wasm execute $ACCOUNT_ADDR "$execute_msg" --from neutron1phx0sz708k3t6xdnyc98hgkyhra4tp44et5s68 $EXECUTE_FLAGS
