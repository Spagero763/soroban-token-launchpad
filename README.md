# soroban-lending-protocol

Overcollateralized lending and borrowing on Soroban.
Dynamic interest rates, Dutch auction liquidations, price oracle abstraction, on-chain risk params.

This is a complete money market implementation — not a simplified demo.

---

## Technical Architecture

### `lending_pool` Contract

The central accounting contract. Tracks reserves and user balances using a
**scaled balance** model: actual balances are stored as `scaled = actual / liquidity_index`,
where `liquidity_index` compounds interest over time. This eliminates the need to iterate
over all users on each interest accrual.

Key storage:
```rust
// Per-reserve state
pub struct ReserveData {
    pub liquidity_index: u128,           // Ray (1e27) scaled
    pub variable_borrow_index: u128,
    pub current_liquidity_rate: u128,
    pub current_variable_borrow_rate: u128,
    pub last_update_timestamp: u64,
    pub a_token_supply: u128,            // Total scaled aToken supply
    pub total_variable_debt: u128,
    pub configuration: ReserveConfiguration,
}

// Per-reserve config (packed into u128 bitmap)
pub struct ReserveConfiguration {
    pub ltv: u16,                        // Max loan-to-value (basis points)
    pub liquidation_threshold: u16,
    pub liquidation_bonus: u16,
    pub decimals: u8,
    pub active: bool,
    pub borrowing_enabled: bool,
    pub reserve_factor: u16,
}
```

Entry points: `supply`, `withdraw`, `borrow`, `repay`, `set_user_use_reserve_as_collateral`.

### `interest_rate_model` Contract

Jump-rate two-slope model:
```
if utilization <= optimal_rate:
    borrow_rate = base_rate + (utilization / optimal_rate) * slope1
else:
    borrow_rate = base_rate + slope1 + ((utilization - optimal_rate) /
                  (1 - optimal_rate)) * slope2
```
Parameters are per-asset and governed by the `collateral_manager` admin.
`slope2` is intentionally steep (e.g., 300% APY at 100% utilization) to
prevent full pool drainage.

### `price_oracle` Contract

Abstraction layer over [Reflector Network](https://reflector.network) TWAP feeds.
- Staleness guard: rejects any price older than `max_age_seconds` (configurable per asset).
- Fallback aggregator: if the primary feed is stale, queries up to 3 backup feeds and
  returns the median.
- Emergency oracle admin can manually post prices with a `circuit_breaker` flag that
  pauses all borrows if triggered.

### `liquidation_engine` Contract

When a user's health factor drops below 1.0, any caller can invoke
`liquidate(collateral_asset, debt_asset, user, debt_to_cover)`.

Protocol uses a **close factor** (max 50% of debt repayable per call) and a
**liquidation bonus** paid to the liquidator in collateral tokens (e.g., 5% bonus means
$105 of collateral seized per $100 of debt repaid). Dutch auction variant available for
large positions — price starts at bonus and rises to max bonus over 30 minutes.

### `collateral_manager` Contract

Tracks `Map<Address, Map<Address, u128>>` (user → asset → scaled_deposit).
`get_account_data(user) -> (total_collateral_usd, total_debt_usd, health_factor)`
iterates over user's active collateral assets, prices each via oracle, applies LTV,
and returns the aggregated health factor.

---
