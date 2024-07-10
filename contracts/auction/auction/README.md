# Auction contract

This is a Dutch auction contract of a pair `(TOKEN_1, TOKEN_2)`, where the sold token is always `TOKEN_1` and bought token is always `TOKEN_2`.

## Dutch auction

In Dutch auction we start the auction with a high price and lower it until the auction ends. The price is lowered by a fixed amount every block, based on the strategy of the auction.

MMs are allowed to bid in the auction based on their preferred price, bids are resolved immediately, as long as there is `TOKEN_1` to be sold.

The auction is finished once the last block of the auction has been reached or all `TOKEN_1` has been sold, whichever comes first.

## Price

Each auction has a starting block and end block, as well as starting price and end price. An auction starts from the starting price that decreases by a fixed amount each block based on the following calculation:

`decrease_price_per_block = (starting_price - end_price) / (end_block - start_block)`

The price of the pair for each block is known when the auction starts. We can use the following calculation to get the price at block `X`:

`price_on_block_X = starting_price - (decrease_price_per_block * (block_X - start_block))`

## Auction strategy
```rust
pub struct AuctionStrategy {
    pub start_price_perc: u64, // BPS
    pub end_price_perc: u64,   // BPS
}
```

The auction strategy is used to calculate the starting price and end price of the auction.

`start_price_perc` is how much percentage (in BPS) to add to the fair price to make the starting price.

`end_price_perc` is how much percentage (IN BPS) to reduce from the fair price to make the end price.

Ex:
Let say our price is `2`, and we set `start_price_perc` to 2000 BPS (20%) and `end_price_perc` to 2000 BPS (20%).

The starting price will be `2 + (2 * 20%) = 2.4` and the end price will be `2 - (2 * 20%) = 1.6`.

## Executables

`AuctionFunds {}` - Send funds to be auctioned during the next auction.

`WithdrawFunds {}` - Withdraw funds sent to the auction. Only funds from pending auctions can be withdrawn.

`Bid {}` - Bid in the active auction. The bid is resolved immediately.

### Doing a bid

First we need to know what the price of the auction before bidding, 2 queries are available:

1. `GetPrice` - This gives us the current price of the auction.
2. `GetAuction` - This gives us the auction `start_price`, `end_price`, `start_block`, `end_block`, which allows us to calculate the price decrease per block, and get the price in any future block. 

Once we know the price and want to bid on that price, we execute `bid {}` message on the auction contract, and provide the amount of `TOKEN_2` we want to buy with, any leftovers will be returned to the bidder.

### Auction management

`FinishAuction { limit: u64 }` - Resolve the current auction if the auction is finished.
We check to make sure the auction is in fact finished, by either no more `TOKEN_1` to sell or the time passed.
Based on the weight each seller had in the auction, we send the amount of `TOKEN_2` each seller should get, and `TOKEN_1` in case we have unsold tokens left.
There will be cases where we have leftover results from rounding, the leftover tokens will be added to the next auction, to mitigate the leftovers of the next auction.
The impact of the leftover tokens is minimal per seller, the loss is less than 1 udenom (1 millionth of 1 token) per auction.

`CleanAfterAuction {}` - Clean up storage from the closed auction that is not needed anymore.

### Admin

The admin of each auction is the Auctions Manager contract, which makes it easier for us to manage multiple auctions.

`PauseAuction {}` - Allows us to pause the auction in case of an emergency.

`ResumeAuction {}` - Resume the auction after it was paused.

`UpdateStrategy { strategy: AuctionStrategy }` - update the strategy of the auction, see more in the [Auction strategy](#auction-strategy) section.

`StartAuction(NewAuctionParams)` - Start a new auction.

The parameter `NewAuctionParams`:
```rust
pub struct NewAuctionParams {
    /// Optional start block, if not provided, it will start from the current block
    pub start_block: Option<u64>,
    /// When auction should end
    pub end_block: u64,
}
```
To start an auction we can provide a start block, if not provided, it will start from the current block, and the end block of the auction.
The price is taken from an oracle.

## Price freshness

The oracles provide us with the price of the pair as well as the time it received this price.
If the price is older than 3 days and 6 hours we consider it stale and we don't start the auction.

In practice, prices will be calculated once per day (when auction is finished), so days means cycle, but freshness calculation happens in timestamp.
We give some spare time of freshness to reflect any possible factors like starting the auction little later in the day.

The price range of the auction is directly influenced by the price freshness, the older the price, the bigger the range.

Ex:
Price is older than 1 day old, we multiple the strategy by 1.5, so if the strategy is to start at 20% above the price, we start at 30% above the price.
Price is older than 2 days old, we multiple the strategy by 2, so if the strategy is to start at 20% above the price, we start at 40% above the price.

Up to a maximum of 75% increase in the starting price.

The price freshness strategy is configurable.
