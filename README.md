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

## 🌊 Drips Wave Program

Register at [drips.network](https://www.drips.network): connect EVM wallet → Claim GitHub
profile → your wallet auto-receives reward streams when your PRs merge.

**Claiming rules:** One issue at a time. Comment to claim. If unsubmitted after the SLA
(7/14/21 days by tier), issue is released.

### Reward Tiers

| Label | Scope | Reward |
|---|---|---|
| `drips:trivial` | Docs, test fixtures, CI, lint | $15 – $50 |
| `drips:medium` | Features, UI components, endpoints, refactors | $100 – $300 |
| `drips:high` | Core math, liquidation logic, oracle security, subsystems | $350 – $1000 |

---

## 📋 Seeded Issues

### `drips:trivial`

**#1 — Add `ReserveConfiguration` bitmap encode/decode unit tests**
`ReserveConfiguration` is packed into a `u128` bitmap for storage efficiency.
The pack/unpack functions have no tests. Add unit tests covering: all fields at
min/max values, round-trip consistency, and confirmed rejection of out-of-range values.

**#2 — Write `calculate_linear_interest` and `calculate_compounded_interest` tests**
Both functions are used in index accrual. Add property-based tests using `proptest`
verifying: zero time delta returns factor of 1.0, positive interest always grows index,
and known fixtures from Aave v3 math spec match.

**#3 — Add `CONTRIBUTING.md` with local test environment setup**
Document: installing Rust/soroban-cli, building contracts, running the indexer locally
with Docker Compose, running the frontend, and how to create a test account on Stellar
testnet with funded balances using Friendbot.

**#4 — Add GitHub Actions matrix CI for all 5 contract crates**
Matrix strategy: `[lending_pool, interest_rate_model, price_oracle, liquidation_engine,
collateral_manager]`. Each job: `cargo test`, `cargo clippy -- -D warnings`,
`cargo build --target wasm32-unknown-unknown --release`. Cache `~/.cargo` between runs.

**#5 — Typegen script: generate TypeScript contract client types from compiled WASM**
Use `soroban contract bindings typescript` (soroban-cli 20+) to autogenerate typed
clients for all 5 contracts. Add an `npm run typegen` script in the repo root that
runs the CLI for each WASM artifact and writes output to `frontend/src/lib/contracts/`.

---

### `drips:medium`

**#6 — Implement `HealthBar` frontend component**
A component that takes `health_factor: number` and renders:
- Gradient fill bar from red (HF < 1) → yellow (1–1.5) → green (> 1.5)
- Numeric display with tooltip explaining liquidation threshold
- Animated pulse when HF < 1.05 (danger zone)
- Must not re-render on unrelated state changes (use `React.memo` correctly)
No third-party chart libs — pure CSS + SVG.

**#7 — Implement `get_account_data` in `collateral_manager`**
Given a `user: Address`, iterate over their active collateral assets (tracked in a
`Vec<Address>` per user), price each asset via oracle, compute:
```
total_collateral_base = Σ (balance_i * price_i * ltv_i)
total_debt_base       = Σ (debt_i * price_i)
health_factor         = total_collateral_base / total_debt_base
```
Return `(total_collateral_base, total_debt_base, available_borrow_base, health_factor)`.
Must handle the zero-debt edge case (return `u128::MAX` health factor).

**#8 — Build liquidation bot script**
`scripts/liquidate/bot.ts`: polls `GET /at-risk-positions` from the indexer every 30s,
filters positions with `health_factor < 1.0`, simulates the liquidation call via
`SorobanRpc.simulateTransaction`, checks if the bonus exceeds estimated gas, and submits
if profitable. Must handle nonce management, fee bumping on timeout, and error logging
to a structured JSON log file.

**#9 — Implement variable debt index accrual in `lending_pool::borrow`**
On each `borrow` call, accrue variable debt index:
```
new_index = old_index * (1 + rate * delta_time)
```
Update `ReserveData.variable_borrow_index` and `ReserveData.last_update_timestamp`.
Recompute `current_variable_borrow_rate` from the interest rate model using new
utilization. Emit `ReserveDataUpdated` event. Numerical tests required.

**#10 — Add `MarketTable` frontend component with sortable columns**
Renders a table of all active lending markets. Columns: Asset, Total Supply (USD),
Total Borrow (USD), Supply APY, Borrow APY, Utilization %, Liquidity.
Sortable by any column. Data sourced from `GET /markets` indexer endpoint.
Must load with skeleton placeholders and handle empty/error states.

**#11 — Implement reserve factor fee collection in `lending_pool`**
A percentage of interest paid by borrowers (`reserve_factor`, e.g. 10%) should be
minted as aTokens to a `treasury_address`. Implement `collect_protocol_fees(asset)` callable
by admin. Must correctly track `accrued_to_treasury` separately from LP earnings.
Add integration test asserting treasury balance grows over simulated time.

**#12 — Build `GET /at-risk-positions` indexer endpoint**
Query the DB for all users with materialized `health_factor < 1.2`. For each, return:
`user_address`, `health_factor`, `total_debt_usd`, `liquidatable_collateral_usd`,
`best_collateral_asset`, `worst_debt_asset`. Paginated, sorted by health_factor ASC.
Must refresh from on-chain state if DB entry is older than 60 seconds.

---

### `drips:high`

**#13 — Implement Dutch auction liquidation in `liquidation_engine`**
For positions with debt > $10,000 USD equivalent, replace instant liquidation with a
Dutch auction. Auction starts at `liquidation_bonus_min` (e.g., 3%) and linearly
increases to `liquidation_bonus_max` (e.g., 12%) over 30 minutes (configurable in ledgers).
Any caller can `bid(auction_id, debt_to_cover)` and receive collateral at the current
bonus rate. Auction closes when debt is fully covered or `expiry_ledger` passes.
Requires `AuctionCreated`, `BidPlaced`, `AuctionClosed` events. Full test suite.

**#14 — Implement `price_oracle` staleness guard with Reflector integration**
Integrate Reflector Network's on-chain oracle contract as the primary price source.
Call `reflector_contract.lastprice(asset)` and validate `(price, timestamp)`.
If `current_ledger_time - timestamp > max_age_seconds`, mark the asset feed as stale
and trigger `circuit_breaker` pause on that asset's borrow market. Implement fallback
to a secondary oracle address if primary is stale. Add a mock oracle contract for
deterministic testing.

**#15 — Implement eMode (efficiency mode) for correlated asset pairs**
eMode allows borrowers to use higher LTV (e.g., 97%) when collateral and debt are
correlated assets (e.g., both are stablecoins). Add `emode_category: u8` to
`ReserveConfiguration`. Add `set_user_emode(category_id)` to `lending_pool`.
Override standard LTV/threshold/bonus with eMode-specific params when both assets
share the same `emode_category`. Must correctly fall back to standard params when
borrowing non-correlated assets. Update health factor calculation. Full audit-ready
test coverage.

---

## 📁 Project Structure
```
soroban-lending-protocol/
├── contracts/
│   ├── lending_pool/src/
│   │   ├── lib.rs              # supply, withdraw, borrow, repay
│   │   ├── reserve.rs          # ReserveData, index accrual logic
│   │   ├── user_account.rs     # scaled balance tracking
│   │   ├── errors.rs
│   │   ├── events.rs
│   │   └── types.rs
│   ├── interest_rate_model/src/
│   │   ├── lib.rs              # get_borrow_rate(), get_liquidity_rate()
│   │   └── jump_rate.rs        # two-slope model math
│   ├── price_oracle/src/
│   │   ├── lib.rs              # get_asset_price(), set_fallback_oracle()
│   │   ├── aggregator.rs       # median fallback logic
│   │   └── types.rs
│   ├── liquidation_engine/src/
│   │   ├── lib.rs              # liquidate(), create_auction(), bid()
│   │   ├── auction.rs          # Dutch auction state machine
│   │   └── types.rs
│   └── collateral_manager/src/
│       └── lib.rs              # get_account_data(), health factor
├── frontend/src/components/
│   ├── BorrowPanel/            # Borrow/repay UI with APY display
│   ├── SupplyPanel/            # Supply/withdraw UI with aToken balance
│   ├── LiquidationDashboard/   # At-risk positions table
│   ├── HealthBar/              # Health factor gradient indicator
│   └── MarketTable/            # Market overview table
├── indexer/src/
│   ├── handlers/               # Supply, Borrow, Repay, Liquidate event handlers
│   ├── db/                     # Schema: reserves, positions, auctions
│   └── api/                    # /markets, /positions/:user, /at-risk-positions
├── scripts/
│   ├── deploy/                 # Deploy + initialize all contracts
│   ├── risk/                   # Risk parameter simulation scripts
│   └── liquidate/              # Automated liquidation bot
└── tests/
    ├── unit/                   # Rust unit tests
    ├── simulation/             # Monte Carlo interest accrual simulations
    └── integration/            # Full borrow → accrue → liquidate flow
```

