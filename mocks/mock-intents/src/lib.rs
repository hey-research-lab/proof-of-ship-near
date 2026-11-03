//! Stands in for the NEAR Intents verifier (`intents.near`) in the sandbox, which has no Intents deployment. It
//! implements only the deposit rule the escrow relies on, as docs.near-intents.org documents it: an `ft_transfer_call`
//! whose `msg` is an account id credits that account, keyed `nep141:<token>`; anything it cannot parse is refused in
//! full. `mt_balance_of` answers like the real verifier's NEP-245 view.

use near_contract_standards::fungible_token::receiver::FungibleTokenReceiver;
use near_sdk::json_types::U128;
use near_sdk::store::LookupMap;
use near_sdk::{env, near, AccountId, BorshStorageKey, PanicOnDefault, PromiseOrValue};

#[derive(BorshStorageKey)]
#[near]
enum StorageKey {
    Balances,
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct MockIntents {
    balances: LookupMap<(AccountId, String), u128>,
    /// When true, every deposit is refused (the receiver panics), to test the escrow's failed-payout path.
    refuse: bool,
}

#[near]
impl MockIntents {
    #[init]
    pub fn new() -> Self {
        Self {
            balances: LookupMap::new(StorageKey::Balances),
            refuse: false,
        }
    }

    pub fn set_refuse(&mut self, refuse: bool) {
        self.refuse = refuse;
    }

    pub fn mt_balance_of(&self, account_id: AccountId, token_id: String) -> U128 {
        U128(self.balances.get(&(account_id, token_id)).copied().unwrap_or(0))
    }
}

#[near]
impl FungibleTokenReceiver for MockIntents {
    fn ft_on_transfer(&mut self, sender_id: AccountId, amount: U128, msg: String) -> PromiseOrValue<U128> {
        if self.refuse {
            env::panic_str("mock intents: refusing deposits");
        }
        let owner: AccountId = if msg.is_empty() {
            sender_id
        } else {
            msg.parse()
                .unwrap_or_else(|_| env::panic_str("mock intents: msg is not an account id"))
        };
        let key = (owner, format!("nep141:{}", env::predecessor_account_id()));
        let prev = self.balances.get(&key).copied().unwrap_or(0);
        self.balances.insert(key, prev + amount.0);
        PromiseOrValue::Value(U128(0))
    }
}
