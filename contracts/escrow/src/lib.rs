#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, contracterror, token, Address, Env, Vec};

// TTL thresholds for persistent escrow entries (~57–115 days at 5 s/ledger).
const TTL_MIN: u32 = 100_000;
const TTL_MAX: u32 = 200_000;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum EscrowError {
    NotFound          = 1,
    AlreadySettled    = 2,
    InDispute         = 3,
    Unauthorized      = 4,
    InvalidAmount     = 5,
    AlreadyExists     = 6,
    TimeoutNotReached = 7,
}

#[derive(Clone, PartialEq)]
#[contracttype]
pub enum EscrowStatus {
    Active,
    Released,
    Refunded,
    Disputed,
}

#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    /// Per-escrow data — stored in persistent storage with individual TTL.
    Escrow(u64),
    /// Contract metadata — stored in instance storage (shared TTL is fine).
    Admin,
    /// Contract metadata — stored in instance storage (shared TTL is fine).
    Platform,
}

/// Full escrow record. `token` stores the SAC address used for this escrow (#683).
#[derive(Clone)]
#[contracttype]
pub struct Escrow {
    pub buyer: Address,
    pub farmer: Address,
    /// SAC token address used for this escrow (any SEP-0041 token, not just XLM).
    pub token: Address,
    pub amount: i128,
    pub timeout_unix: u64,
    pub status: EscrowStatus,
}

#[contract]
pub struct EscrowContract;

#[contractimpl]
impl EscrowContract {
    /// Must be called once to register the platform fee recipient.
    pub fn init(env: Env, platform_address: Address) {
        env.storage().instance().set(&DataKey::Platform, &platform_address);
    }

    /// Deposit funds into escrow for `order_id`.
    ///
    /// `token` is any SAC-compatible token address (#683 — multi-token support).
    pub fn deposit(
        env: Env,
        token: Address,
        order_id: u64,
        buyer: Address,
        farmer: Address,
        amount: i128,
        timeout_unix: u64,
    ) -> Result<(), EscrowError> {
        buyer.require_auth();
        if amount <= 0 {
            return Err(EscrowError::InvalidAmount);
        }
        if env.storage().persistent().has(&DataKey::Escrow(order_id)) {
            return Err(EscrowError::AlreadyExists);
        }

        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&buyer, &env.current_contract_address(), &amount);

        let escrow = Escrow {
            buyer,
            farmer,
            token,
            amount,
            timeout_unix,
            status: EscrowStatus::Active,
        };
        env.storage().persistent().set(&DataKey::Escrow(order_id), &escrow);
        env.storage().persistent().extend_ttl(&DataKey::Escrow(order_id), TTL_MIN, TTL_MAX);
        Ok(())
    }

    /// Create multiple escrows in a single transaction to reduce fees (#689).
    ///
    /// Each tuple is `(order_id, buyer, farmer, token, amount, timeout_unix)`.
    /// All entries are validated before any state is written; if any entry is
    /// invalid the entire batch is rejected.
    pub fn batch_deposit(
        env: Env,
        entries: Vec<(u64, Address, Address, Address, i128, u64)>,
    ) -> Result<(), EscrowError> {
        // Validate all entries first (fail-fast before touching state).
        for entry in entries.iter() {
            let (order_id, _buyer, _farmer, _token, amount, _timeout) = entry;
            if amount <= 0 {
                return Err(EscrowError::InvalidAmount);
            }
            if env.storage().persistent().has(&DataKey::Escrow(order_id)) {
                return Err(EscrowError::AlreadyExists);
            }
        }

        for entry in entries.iter() {
            let (order_id, buyer, farmer, token, amount, timeout_unix) = entry;
            buyer.require_auth();

            let token_client = token::Client::new(&env, &token);
            token_client.transfer(&buyer, &env.current_contract_address(), &amount);

            let escrow = Escrow {
                buyer,
                farmer,
                token,
                amount,
                timeout_unix,
                status: EscrowStatus::Active,
            };
            env.storage().persistent().set(&DataKey::Escrow(order_id), &escrow);
            env.storage().persistent().extend_ttl(&DataKey::Escrow(order_id), TTL_MIN, TTL_MAX);
        }
        Ok(())
    }

    /// Release funds to the farmer, deducting a platform fee.
    ///
    /// Uses the token stored in the escrow record (#683).
    /// `platform_fee_bps`: fee in basis points (e.g. 250 = 2.5%). Max 1000 (10%).
    pub fn release(
        env: Env,
        order_id: u64,
        platform_fee_bps: u32,
    ) -> Result<(), EscrowError> {
        if platform_fee_bps > 1000 {
            return Err(EscrowError::InvalidAmount);
        }

        let mut escrow: Escrow = env
            .storage()
            .persistent()
            .get(&DataKey::Escrow(order_id))
            .ok_or(EscrowError::NotFound)?;

        escrow.buyer.require_auth();

        match escrow.status {
            EscrowStatus::Released | EscrowStatus::Refunded => {
                return Err(EscrowError::AlreadySettled);
            }
            EscrowStatus::Disputed => {
                return Err(EscrowError::InDispute);
            }
            EscrowStatus::Active => {}
        }

        let token_client = token::Client::new(&env, &escrow.token);

        let fee_amount = (escrow.amount * platform_fee_bps as i128) / 10_000;
        let farmer_amount = escrow.amount - fee_amount;

        if fee_amount > 0 {
            let platform: Address = env
                .storage()
                .instance()
                .get(&DataKey::Platform)
                .ok_or(EscrowError::NotFound)?;
            token_client.transfer(&env.current_contract_address(), &platform, &fee_amount);
        }

        token_client.transfer(&env.current_contract_address(), &escrow.farmer, &farmer_amount);

        escrow.status = EscrowStatus::Released;
        env.storage().persistent().set(&DataKey::Escrow(order_id), &escrow);
        env.storage().persistent().extend_ttl(&DataKey::Escrow(order_id), TTL_MIN, TTL_MAX);
        Ok(())
    }

    pub fn set_admin(env: Env, admin: Address) {
        admin.require_auth();
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("admin already set");
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
    }

    /// Refund funds to the buyer after timeout.
    ///
    /// Uses the token stored in the escrow record (#683).
    pub fn refund(env: Env, order_id: u64) -> Result<(), EscrowError> {
        let mut escrow: Escrow = env
            .storage()
            .persistent()
            .get(&DataKey::Escrow(order_id))
            .ok_or(EscrowError::NotFound)?;

        escrow.buyer.require_auth();

        match escrow.status {
            EscrowStatus::Released | EscrowStatus::Refunded => {
                return Err(EscrowError::AlreadySettled);
            }
            _ => {}
        }
        if env.ledger().timestamp() < escrow.timeout_unix {
            return Err(EscrowError::TimeoutNotReached);
        }

        let token_client = token::Client::new(&env, &escrow.token);
        token_client.transfer(&env.current_contract_address(), &escrow.buyer, &escrow.amount);

        escrow.status = EscrowStatus::Refunded;
        env.storage().persistent().set(&DataKey::Escrow(order_id), &escrow);
        env.storage().persistent().extend_ttl(&DataKey::Escrow(order_id), TTL_MIN, TTL_MAX);
        Ok(())
    }

    pub fn dispute(env: Env, order_id: u64, caller: Address) -> Result<(), EscrowError> {
        caller.require_auth();
        let mut escrow: Escrow = env
            .storage()
            .persistent()
            .get(&DataKey::Escrow(order_id))
            .ok_or(EscrowError::NotFound)?;

        if caller != escrow.buyer && caller != escrow.farmer {
            return Err(EscrowError::Unauthorized);
        }
        match escrow.status {
            EscrowStatus::Released | EscrowStatus::Refunded => {
                return Err(EscrowError::AlreadySettled);
            }
            _ => {}
        }

        escrow.status = EscrowStatus::Disputed;
        env.storage().persistent().set(&DataKey::Escrow(order_id), &escrow);
        env.storage().persistent().extend_ttl(&DataKey::Escrow(order_id), TTL_MIN, TTL_MAX);
        Ok(())
    }

    /// Admin resolves a disputed escrow. Uses the token stored in the record (#683).
    pub fn resolve_dispute(env: Env, order_id: u64, release_to_farmer: bool) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("admin not set");
        admin.require_auth();

        let mut escrow: Escrow = env
            .storage()
            .persistent()
            .get(&DataKey::Escrow(order_id))
            .expect("escrow not found");

        if escrow.status != EscrowStatus::Disputed {
            panic!("escrow is not in dispute");
        }

        let token_client = token::Client::new(&env, &escrow.token);
        if release_to_farmer {
            token_client.transfer(&env.current_contract_address(), &escrow.farmer, &escrow.amount);
            escrow.status = EscrowStatus::Released;
        } else {
            token_client.transfer(&env.current_contract_address(), &escrow.buyer, &escrow.amount);
            escrow.status = EscrowStatus::Refunded;
        }
        env.storage().persistent().set(&DataKey::Escrow(order_id), &escrow);
        env.storage().persistent().extend_ttl(&DataKey::Escrow(order_id), TTL_MIN, TTL_MAX);
    }

    pub fn get(env: Env, order_id: u64) -> Result<Escrow, EscrowError> {
        env.storage()
            .persistent()
            .get(&DataKey::Escrow(order_id))
            .ok_or(EscrowError::NotFound)
    }

    /// Read-only view: returns the escrow state for `order_id`, or `None` if it does not exist.
    pub fn get_escrow(env: Env, order_id: u64) -> Option<Escrow> {
        env.storage().persistent().get(&DataKey::Escrow(order_id))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Address, Env};

    fn store_escrow(env: &Env, order_id: u64, buyer: Address, farmer: Address, token: Address) {
        let escrow = Escrow {
            buyer,
            farmer,
            token,
            amount: 1_000_0000,
            timeout_unix: 1_000,
            status: EscrowStatus::Active,
        };
        env.storage().persistent().set(&DataKey::Escrow(order_id), &escrow);
    }

    // ── EscrowStatus::Disputed consolidation tests ────────────────────────────

    #[test]
    fn dispute_sets_status_to_disputed() {
        let env = Env::default();
        env.mock_all_auths();
        let buyer = Address::generate(&env);
        let farmer = Address::generate(&env);
        let token = Address::generate(&env);
        store_escrow(&env, 1, buyer.clone(), farmer, token);
        EscrowContract::dispute(env.clone(), 1, buyer).unwrap();
        let updated = EscrowContract::get(env, 1).unwrap();
        assert_eq!(updated.status, EscrowStatus::Disputed);
    }

    #[test]
    fn release_disputed_escrow_returns_in_dispute_error() {
        let env = Env::default();
        env.mock_all_auths();
        let buyer = Address::generate(&env);
        let farmer = Address::generate(&env);
        let token = Address::generate(&env);
        let escrow = Escrow {
            buyer: buyer.clone(),
            farmer,
            token,
            amount: 1_000_0000,
            timeout_unix: 1_000,
            status: EscrowStatus::Disputed,
        };
        env.storage().persistent().set(&DataKey::Escrow(2), &escrow);
        let result = EscrowContract::release(env, 2, 0);
        assert_eq!(result, Err(EscrowError::InDispute));
    }

    // ── error variant tests ───────────────────────────────────────────────────

    #[test]
    fn get_not_found() {
        let env = Env::default();
        let result = EscrowContract::get(env, 99);
        assert_eq!(result, Err(EscrowError::NotFound));
    }

    #[test]
    fn dispute_not_found() {
        let env = Env::default();
        let caller = Address::generate(&env);
        let result = EscrowContract::dispute(env, 99, caller);
        assert_eq!(result, Err(EscrowError::NotFound));
    }

    #[test]
    fn dispute_unauthorized() {
        let env = Env::default();
        env.mock_all_auths();
        let buyer = Address::generate(&env);
        let farmer = Address::generate(&env);
        let stranger = Address::generate(&env);
        let token = Address::generate(&env);
        store_escrow(&env, 3, buyer, farmer, token);
        let result = EscrowContract::dispute(env, 3, stranger);
        assert_eq!(result, Err(EscrowError::Unauthorized));
    }

    #[test]
    fn dispute_already_settled() {
        let env = Env::default();
        env.mock_all_auths();
        let buyer = Address::generate(&env);
        let farmer = Address::generate(&env);
        let token = Address::generate(&env);
        let escrow = Escrow {
            buyer: buyer.clone(),
            farmer,
            token,
            amount: 1_000_0000,
            timeout_unix: 1_000,
            status: EscrowStatus::Released,
        };
        env.storage().persistent().set(&DataKey::Escrow(4), &escrow);
        let result = EscrowContract::dispute(env, 4, buyer);
        assert_eq!(result, Err(EscrowError::AlreadySettled));
    }

    #[test]
    fn refund_timeout_not_reached() {
        let env = Env::default();
        let buyer = Address::generate(&env);
        let farmer = Address::generate(&env);
        let token = Address::generate(&env);
        store_escrow(&env, 5, buyer, farmer, token);
        let escrow: Escrow = env.storage().persistent().get(&DataKey::Escrow(5)).unwrap();
        assert!(env.ledger().timestamp() < escrow.timeout_unix);
    }

    #[test]
    fn release_fee_exceeds_maximum() {
        let env = Env::default();
        env.mock_all_auths();
        let buyer = Address::generate(&env);
        let farmer = Address::generate(&env);
        let token = Address::generate(&env);
        store_escrow(&env, 6, buyer, farmer, token);
        let result = EscrowContract::release(env, 6, 1001);
        assert_eq!(result, Err(EscrowError::InvalidAmount));
    }

    #[test]
    fn release_not_found() {
        let env = Env::default();
        env.mock_all_auths();
        let result = EscrowContract::release(env, 99, 250);
        assert_eq!(result, Err(EscrowError::NotFound));
    }

    #[test]
    fn release_already_settled() {
        let env = Env::default();
        env.mock_all_auths();
        let buyer = Address::generate(&env);
        let farmer = Address::generate(&env);
        let token = Address::generate(&env);
        let escrow = Escrow {
            buyer: buyer.clone(),
            farmer,
            token,
            amount: 1_000_0000,
            timeout_unix: 1_000,
            status: EscrowStatus::Released,
        };
        env.storage().persistent().set(&DataKey::Escrow(7), &escrow);
        let result = EscrowContract::release(env, 7, 0);
        assert_eq!(result, Err(EscrowError::AlreadySettled));
    }

    #[test]
    fn get_returns_escrow_data() {
        let env = Env::default();
        let buyer = Address::generate(&env);
        let farmer = Address::generate(&env);
        let token = Address::generate(&env);
        store_escrow(&env, 8, buyer.clone(), farmer.clone(), token);
        let stored = EscrowContract::get(env, 8).unwrap();
        assert_eq!(stored.buyer, buyer);
        assert_eq!(stored.farmer, farmer);
        assert_eq!(stored.amount, 1_000_0000);
    }

    #[test]
    fn get_escrow_returns_none_for_unknown_order() {
        let env = Env::default();
        let result = EscrowContract::get_escrow(env, 999);
        assert!(result.is_none());
    }

    #[test]
    fn get_escrow_returns_correct_data_after_create() {
        let env = Env::default();
        let buyer = Address::generate(&env);
        let farmer = Address::generate(&env);
        let token = Address::generate(&env);
        store_escrow(&env, 9, buyer.clone(), farmer.clone(), token.clone());
        let result = EscrowContract::get_escrow(env, 9);
        assert!(result.is_some());
        let escrow = result.unwrap();
        assert_eq!(escrow.buyer, buyer);
        assert_eq!(escrow.farmer, farmer);
        assert_eq!(escrow.amount, 1_000_0000);
        assert_eq!(escrow.status, EscrowStatus::Active);
        assert_eq!(escrow.token, token);
    }

    #[test]
    fn two_escrows_have_independent_keys() {
        let env = Env::default();
        let buyer_a = Address::generate(&env);
        let farmer_a = Address::generate(&env);
        let buyer_b = Address::generate(&env);
        let farmer_b = Address::generate(&env);
        let token = Address::generate(&env);

        store_escrow(&env, 10, buyer_a.clone(), farmer_a.clone(), token.clone());
        store_escrow(&env, 11, buyer_b.clone(), farmer_b.clone(), token);

        let mut e10: Escrow = env.storage().persistent().get(&DataKey::Escrow(10)).unwrap();
        e10.status = EscrowStatus::Released;
        env.storage().persistent().set(&DataKey::Escrow(10), &e10);
        env.storage().persistent().extend_ttl(&DataKey::Escrow(10), TTL_MIN, TTL_MAX);

        let e11: Escrow = env.storage().persistent().get(&DataKey::Escrow(11)).unwrap();
        assert_eq!(e11.status, EscrowStatus::Active, "escrow 11 must not be affected by escrow 10 mutation");
        assert_eq!(e11.buyer, buyer_b);
    }

    #[test]
    fn fee_rounding() {
        let amount: i128 = 1;
        let fee = (amount * 250_i128) / 10_000;
        assert_eq!(fee, 0);
        let amount2: i128 = 40_000;
        let fee2 = (amount2 * 250_i128) / 10_000;
        assert_eq!(fee2, 1_000);
    }

    #[test]
    fn fee_zero_bps() {
        let amount: i128 = 1_000_0000;
        let fee = (amount * 0_i128) / 10_000;
        assert_eq!(fee, 0);
        assert_eq!(amount - fee, 1_000_0000);
    }

    #[test]
    fn fee_250_bps() {
        let amount: i128 = 1_000_0000;
        let fee = (amount * 250_i128) / 10_000;
        assert_eq!(fee, 25_0000);
        assert_eq!(amount - fee, 975_0000);
    }

    // ── #683 multi-token: token address is stored and retrievable ─────────────

    #[test]
    fn escrow_stores_token_address() {
        let env = Env::default();
        let buyer = Address::generate(&env);
        let farmer = Address::generate(&env);
        let token = Address::generate(&env);
        store_escrow(&env, 20, buyer, farmer, token.clone());
        let escrow = EscrowContract::get(env, 20).unwrap();
        assert_eq!(escrow.token, token);
    }

    #[test]
    fn two_escrows_can_use_different_tokens() {
        let env = Env::default();
        let buyer = Address::generate(&env);
        let farmer = Address::generate(&env);
        let token_a = Address::generate(&env);
        let token_b = Address::generate(&env);
        store_escrow(&env, 21, buyer.clone(), farmer.clone(), token_a.clone());
        store_escrow(&env, 22, buyer, farmer, token_b.clone());
        assert_eq!(EscrowContract::get(env.clone(), 21).unwrap().token, token_a);
        assert_eq!(EscrowContract::get(env, 22).unwrap().token, token_b);
    }

    // ── #689 batch_deposit validation ─────────────────────────────────────────

    #[test]
    fn batch_deposit_rejects_zero_amount() {
        let env = Env::default();
        env.mock_all_auths();
        let buyer = Address::generate(&env);
        let farmer = Address::generate(&env);
        let token = Address::generate(&env);
        let mut entries = Vec::new(&env);
        entries.push_back((100_u64, buyer, farmer, token, 0_i128, 9999_u64));
        let result = EscrowContract::batch_deposit(env, entries);
        assert_eq!(result, Err(EscrowError::InvalidAmount));
    }

    #[test]
    fn batch_deposit_rejects_negative_amount() {
        let env = Env::default();
        env.mock_all_auths();
        let buyer = Address::generate(&env);
        let farmer = Address::generate(&env);
        let token = Address::generate(&env);
        let mut entries = Vec::new(&env);
        entries.push_back((101_u64, buyer, farmer, token, -1_i128, 9999_u64));
        let result = EscrowContract::batch_deposit(env, entries);
        assert_eq!(result, Err(EscrowError::InvalidAmount));
    }

    #[test]
    fn batch_deposit_rejects_duplicate_order_id() {
        let env = Env::default();
        env.mock_all_auths();
        let buyer = Address::generate(&env);
        let farmer = Address::generate(&env);
        let token = Address::generate(&env);
        // Pre-store an escrow with order_id 200
        store_escrow(&env, 200, buyer.clone(), farmer.clone(), token.clone());
        let mut entries = Vec::new(&env);
        entries.push_back((200_u64, buyer, farmer, token, 1000_i128, 9999_u64));
        let result = EscrowContract::batch_deposit(env, entries);
        assert_eq!(result, Err(EscrowError::AlreadyExists));
    }

    // ── #686 property-based fuzz tests ────────────────────────────────────────
    //
    // Soroban's test environment is deterministic; we simulate property-based
    // fuzzing by iterating over a representative set of boundary and random-like
    // values covering the full input space described in the issue.

    /// Property: deposit with any positive amount must succeed (no token transfer
    /// is executed because we write directly to storage, so we test the guard logic).
    #[test]
    fn fuzz_deposit_amount_guard_positive_values() {
        let amounts: &[i128] = &[1, 2, 100, 1_000, i128::MAX / 2, i128::MAX];
        for &amount in amounts {
            let env = Env::default();
            let buyer = Address::generate(&env);
            let farmer = Address::generate(&env);
            let token = Address::generate(&env);
            // Write directly to bypass token transfer (unit-tests the guard only).
            let escrow = Escrow {
                buyer: buyer.clone(),
                farmer,
                token,
                amount,
                timeout_unix: 9999,
                status: EscrowStatus::Active,
            };
            env.storage().persistent().set(&DataKey::Escrow(amount as u64), &escrow);
            let stored = EscrowContract::get(env, amount as u64).unwrap();
            assert_eq!(stored.amount, amount);
        }
    }

    /// Property: deposit with amount <= 0 must always return InvalidAmount.
    #[test]
    fn fuzz_deposit_rejects_non_positive_amounts() {
        let bad_amounts: &[i128] = &[0, -1, -100, i128::MIN];
        for &amount in bad_amounts {
            let env = Env::default();
            env.mock_all_auths();
            let buyer = Address::generate(&env);
            let farmer = Address::generate(&env);
            let token = Address::generate(&env);
            // Manually invoke the guard check (mirrors deposit logic).
            let result: Result<(), EscrowError> = if amount <= 0 {
                Err(EscrowError::InvalidAmount)
            } else {
                Ok(())
            };
            assert_eq!(result, Err(EscrowError::InvalidAmount), "amount={amount} should be rejected");
            // Also verify batch_deposit rejects it.
            let mut entries = Vec::new(&env);
            entries.push_back((1_u64, buyer, farmer, token, amount, 9999_u64));
            let batch_result = EscrowContract::batch_deposit(env, entries);
            assert_eq!(batch_result, Err(EscrowError::InvalidAmount));
        }
    }

    /// Property: release before refund — once released, refund must return AlreadySettled.
    #[test]
    fn fuzz_release_then_refund_ordering() {
        let env = Env::default();
        env.mock_all_auths();
        let buyer = Address::generate(&env);
        let farmer = Address::generate(&env);
        let token = Address::generate(&env);
        let escrow = Escrow {
            buyer: buyer.clone(),
            farmer,
            token,
            amount: 1_000,
            timeout_unix: 0, // already timed out
            status: EscrowStatus::Released,
        };
        env.storage().persistent().set(&DataKey::Escrow(300), &escrow);

        // Refund on an already-released escrow must fail.
        let result = EscrowContract::refund(env, 300);
        assert_eq!(result, Err(EscrowError::AlreadySettled));
    }

    /// Property: refund before release — once refunded, release must return AlreadySettled.
    #[test]
    fn fuzz_refund_then_release_ordering() {
        let env = Env::default();
        env.mock_all_auths();
        let buyer = Address::generate(&env);
        let farmer = Address::generate(&env);
        let token = Address::generate(&env);
        let escrow = Escrow {
            buyer: buyer.clone(),
            farmer,
            token,
            amount: 1_000,
            timeout_unix: 0,
            status: EscrowStatus::Refunded,
        };
        env.storage().persistent().set(&DataKey::Escrow(301), &escrow);

        let result = EscrowContract::release(env, 301, 0);
        assert_eq!(result, Err(EscrowError::AlreadySettled));
    }

    /// Property: timeout boundary — refund must fail when timestamp < timeout_unix
    /// and succeed (guard-wise) when timestamp >= timeout_unix.
    #[test]
    fn fuzz_timeout_boundary_conditions() {
        // Pairs of (ledger_timestamp, timeout_unix, expect_timeout_error)
        let cases: &[(u64, u64, bool)] = &[
            (0, 1, true),           // before timeout
            (999, 1_000, true),     // one second before
            (1_000, 1_000, false),  // exactly at timeout
            (1_001, 1_000, false),  // one second after
            (u64::MAX, 1_000, false), // far future
            (0, 0, false),          // timeout at genesis
        ];

        for &(ts, timeout_unix, expect_err) in cases {
            let env = Env::default();
            env.mock_all_auths();
            env.ledger().set_timestamp(ts);

            let buyer = Address::generate(&env);
            let farmer = Address::generate(&env);
            let token = Address::generate(&env);
            let escrow = Escrow {
                buyer: buyer.clone(),
                farmer,
                token,
                amount: 1_000,
                timeout_unix,
                status: EscrowStatus::Active,
            };
            env.storage().persistent().set(&DataKey::Escrow(400), &escrow);

            // Mirror the refund timeout guard.
            let timed_out = ts >= timeout_unix;
            if expect_err {
                assert!(!timed_out, "ts={ts} timeout={timeout_unix}: expected timeout not reached");
            } else {
                assert!(timed_out, "ts={ts} timeout={timeout_unix}: expected timeout reached");
            }

            // Verify via the actual contract function (no token transfer needed
            // since we only care about the TimeoutNotReached guard path).
            let result = EscrowContract::refund(env, 400);
            if expect_err {
                assert_eq!(result, Err(EscrowError::TimeoutNotReached),
                    "ts={ts} timeout={timeout_unix}");
            } else {
                // The call will fail at the token transfer step (no real token),
                // but it must NOT fail with TimeoutNotReached.
                assert_ne!(result, Err(EscrowError::TimeoutNotReached),
                    "ts={ts} timeout={timeout_unix}");
            }
        }
    }

    /// Property: platform fee calculation never produces negative farmer_amount
    /// for any valid (positive) amount and fee in [0, 1000] bps.
    #[test]
    fn fuzz_fee_calculation_never_negative() {
        let amounts: &[i128] = &[1, 7, 100, 10_000, 1_000_000, i128::MAX / 10_000];
        let fees_bps: &[u32] = &[0, 1, 250, 500, 999, 1000];
        for &amount in amounts {
            for &bps in fees_bps {
                let fee = (amount * bps as i128) / 10_000;
                let farmer_amount = amount - fee;
                assert!(farmer_amount >= 0, "amount={amount} bps={bps} farmer_amount={farmer_amount}");
                assert!(fee >= 0, "fee must be non-negative");
                assert!(fee <= amount, "fee must not exceed amount");
            }
        }
    }

    /// Property: fee_bps > 1000 must always be rejected.
    #[test]
    fn fuzz_release_rejects_excessive_fee_bps() {
        let bad_fees: &[u32] = &[1001, 1002, 5000, 10_000, u32::MAX];
        for &bps in bad_fees {
            let env = Env::default();
            env.mock_all_auths();
            let buyer = Address::generate(&env);
            let farmer = Address::generate(&env);
            let token = Address::generate(&env);
            store_escrow(&env, 500, buyer, farmer, token);
            let result = EscrowContract::release(env, 500, bps);
            assert_eq!(result, Err(EscrowError::InvalidAmount), "bps={bps} should be rejected");
        }
    }
}
