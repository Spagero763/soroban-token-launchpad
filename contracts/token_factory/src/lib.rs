#![no_std]
use soroban_sdk::{contract, contractimpl, Address, Env, String};

#[contract]
pub struct TokenFactory;

#[contractimpl]
impl TokenFactory {
    /// Deploy a new SEP-41 token instance.
    /// Returns the contract ID of the deployed token.
    pub fn create_token(
        _env: Env,
        _name: String,
        _symbol: String,
        _decimals: u32,
        _initial_supply: u128,
        _admin: Address,
    ) -> Address {
        todo!("Implement token deployment via env.deployer()")
    }

    /// Returns all token contract IDs created by this factory.
    pub fn get_tokens(_env: Env) -> soroban_sdk::Vec<Address> {
        todo!("Implement registry lookup")
    }
}
```

Do the same minimal stub pattern for `token_template` and `registry`.

**`.gitignore`**:
```
target/
node_modules/
.env
.DS_Store
*.wasm
dist/
.next/