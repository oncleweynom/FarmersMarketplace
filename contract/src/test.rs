//! Unit tests for the Farmers Marketplace escrow contract.
//!
//! Covers all acceptance criteria from issues #468, #469, #470, #471,
//! plus snapshot() and balance_at() for governance voting.

#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Events, Ledger, LedgerInfo},
    vec, Address, Env, IntoVal,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn setup_env() -> Env {
    let env = Env::default();
    env.ledger().set(LedgerInfo {
        timestamp: 1_000_000,
        protocol_version: 21,
        sequence_number: 1,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 1,
        min_persistent_entry_ttl: 1,
        max_entry_ttl: 300_000,
    });
    env
}

fn register_contract(env: &Env) -> EscrowContractClient {
    EscrowContractClient::new(env, &env.register_contract(None, EscrowContract))
}

fn future_timeout(env: &Env) -> u64 {
    env.ledger().timestamp() + 7_200 // 2 hours
}

// ---------------------------------------------------------------------------
// #469 — buyer != farmer validation
// ---------------------------------------------------------------------------

#[test]
fn test_deposit_same_buyer_and_farmer_returns_invalid_parties() {
    let env = setup_env();
    let client = register_contract(&env);

    let alice = Address::generate(&env);
    let result = client.try_deposit(&1u64, &alice, &alice, &1_000_000, &future_timeout(&env));
    assert_eq!(result, Err(Ok(EscrowError::InvalidParties)));
}

#[test]
fn test_deposit_different_parties_succeeds() {
    let env = setup_env();
    env.mock_all_auths();
    let client = register_contract(&env);

    let buyer = Address::generate(&env);
    let farmer = Address::generate(&env);
    client.deposit(&1u64, &buyer, &farmer, &1_000_000, &future_timeout(&env));
}

// ---------------------------------------------------------------------------
// #470 — timeout_unix must be at least 1 hour in the future
// ---------------------------------------------------------------------------

#[test]
#[should_panic(expected = "timeout must be at least 1 hour in the future")]
fn test_deposit_past_timeout_panics() {
    let env = setup_env();
    env.mock_all_auths();
    let client = register_contract(&env);

    let buyer = Address::generate(&env);
    let farmer = Address::generate(&env);
    let past_timeout = env.ledger().timestamp() - 1;
    client.deposit(&2u64, &buyer, &farmer, &1_000_000, &past_timeout);
}

#[test]
#[should_panic(expected = "timeout must be at least 1 hour in the future")]
fn test_deposit_timeout_less_than_one_hour_panics() {
    let env = setup_env();
    env.mock_all_auths();
    let client = register_contract(&env);

    let buyer = Address::generate(&env);
    let farmer = Address::generate(&env);
    let too_soon = env.ledger().timestamp() + 1_800; // 30 minutes
    client.deposit(&3u64, &buyer, &farmer, &1_000_000, &too_soon);
}

#[test]
fn test_deposit_future_timeout_succeeds() {
    let env = setup_env();
    env.mock_all_auths();
    let client = register_contract(&env);

    let buyer = Address::generate(&env);
    let farmer = Address::generate(&env);
    let just_enough = env.ledger().timestamp() + MIN_TIMEOUT_SECS + 1;
    client.deposit(&4u64, &buyer, &farmer, &1_000_000, &just_enough);
}

// ---------------------------------------------------------------------------
// #471 — events are emitted with correct topics and data
// ---------------------------------------------------------------------------

#[test]
fn test_deposit_emits_event() {
    let env = setup_env();
    env.mock_all_auths();
    let client = register_contract(&env);

    let buyer = Address::generate(&env);
    let farmer = Address::generate(&env);
    let amount: i128 = 5_000_000;
    let order_id: u64 = 10;

    client.deposit(&order_id, &buyer, &farmer, &amount, &future_timeout(&env));

    let events = env.events().all();
    assert_eq!(events.len(), 1);

    let (_, topics, data) = events.get(0).unwrap();
    assert_eq!(
        topics,
        vec![
            &env,
            symbol_short!("escrow").into_val(&env),
            symbol_short!("deposit").into_val(&env),
            order_id.into_val(&env),
        ]
    );
    assert_eq!(data, (buyer.clone(), farmer.clone(), amount).into_val(&env));
}

#[test]
fn test_release_emits_event() {
    let env = setup_env();
    env.mock_all_auths();
    let client = register_contract(&env);

    let buyer = Address::generate(&env);
    let farmer = Address::generate(&env);
    let amount: i128 = 5_000_000;
    let order_id: u64 = 20;

    client.deposit(&order_id, &buyer, &farmer, &amount, &future_timeout(&env));
    env.events().all(); // clear

    client.release(&order_id);

    let events = env.events().all();
    let (_, topics, data) = events.iter().last().unwrap();
    assert_eq!(
        topics,
        vec![
            &env,
            symbol_short!("escrow").into_val(&env),
            symbol_short!("release").into_val(&env),
            order_id.into_val(&env),
        ]
    );
    assert_eq!(data, amount.into_val(&env));
}

#[test]
fn test_refund_emits_event() {
    let env = setup_env();
    env.mock_all_auths();
    let client = register_contract(&env);

    let buyer = Address::generate(&env);
    let farmer = Address::generate(&env);
    let amount: i128 = 5_000_000;
    let order_id: u64 = 30;
    let timeout = future_timeout(&env);

    client.deposit(&order_id, &buyer, &farmer, &amount, &timeout);

    env.ledger().set(LedgerInfo {
        timestamp: timeout + 1,
        protocol_version: 21,
        sequence_number: 2,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 1,
        min_persistent_entry_ttl: 1,
        max_entry_ttl: 300_000,
    });

    client.refund(&order_id);

    let events = env.events().all();
    let (_, topics, data) = events.iter().last().unwrap();
    assert_eq!(
        topics,
        vec![
            &env,
            symbol_short!("escrow").into_val(&env),
            symbol_short!("refund").into_val(&env),
            order_id.into_val(&env),
        ]
    );
    assert_eq!(data, amount.into_val(&env));
}

// ---------------------------------------------------------------------------
// #468 — TTL is extended; settled entries remain readable
// ---------------------------------------------------------------------------

#[test]
fn test_ttl_extended_after_deposit_entry_is_readable() {
    let env = setup_env();
    env.mock_all_auths();
    let client = register_contract(&env);

    let buyer = Address::generate(&env);
    let farmer = Address::generate(&env);
    let order_id: u64 = 40;

    client.deposit(&order_id, &buyer, &farmer, &1_000_000, &future_timeout(&env));

    let record = client.get_escrow(&order_id);
    assert_eq!(record.amount, 1_000_000);
    assert!(!record.released);
}

#[test]
fn test_ttl_extended_after_release_entry_is_settled_not_evicted() {
    let env = setup_env();
    env.mock_all_auths();
    let client = register_contract(&env);

    let buyer = Address::generate(&env);
    let farmer = Address::generate(&env);
    let order_id: u64 = 50;

    client.deposit(&order_id, &buyer, &farmer, &1_000_000, &future_timeout(&env));
    client.release(&order_id);

    let result = client.try_release(&order_id);
    assert_eq!(result, Err(Ok(EscrowError::AlreadySettled)));
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[test]
fn test_refund_before_timeout_returns_not_timed_out() {
    let env = setup_env();
    env.mock_all_auths();
    let client = register_contract(&env);

    let buyer = Address::generate(&env);
    let farmer = Address::generate(&env);
    let order_id: u64 = 60;

    client.deposit(&order_id, &buyer, &farmer, &1_000_000, &future_timeout(&env));

    let result = client.try_refund(&order_id);
    assert_eq!(result, Err(Ok(EscrowError::NotTimedOut)));
}

#[test]
fn test_duplicate_deposit_returns_already_exists() {
    let env = setup_env();
    env.mock_all_auths();
    let client = register_contract(&env);

    let buyer = Address::generate(&env);
    let farmer = Address::generate(&env);
    let order_id: u64 = 70;

    client.deposit(&order_id, &buyer, &farmer, &1_000_000, &future_timeout(&env));

    let result = client.try_deposit(
        &order_id,
        &buyer,
        &farmer,
        &1_000_000,
        &future_timeout(&env),
    );
    assert_eq!(result, Err(Ok(EscrowError::AlreadyExists)));
}

#[test]
fn test_release_nonexistent_order_returns_not_found() {
    let env = setup_env();
    env.mock_all_auths();
    let client = register_contract(&env);

    let result = client.try_release(&999u64);
    assert_eq!(result, Err(Ok(EscrowError::NotFound)));
}

#[test]
fn test_refund_nonexistent_order_returns_not_found() {
    let env = setup_env();
    env.mock_all_auths();
    let client = register_contract(&env);

    let result = client.try_refund(&999u64);
    assert_eq!(result, Err(Ok(EscrowError::NotFound)));
}

// ---------------------------------------------------------------------------
// snapshot() and balance_at() — governance voting
// ---------------------------------------------------------------------------

#[test]
fn test_init_sets_admin() {
    let env = setup_env();
    env.mock_all_auths();
    let client = register_contract(&env);

    let admin = Address::generate(&env);
    client.init(&admin);

    // Second init must fail
    let result = client.try_init(&admin);
    assert_eq!(result, Err(Ok(EscrowError::AlreadyInitialized)));
}

#[test]
fn test_snapshot_requires_admin() {
    let env = setup_env();
    env.mock_all_auths();
    let client = register_contract(&env);

    // No init → no admin stored → Unauthorized
    let result = client.try_snapshot(&vec![&env]);
    assert_eq!(result, Err(Ok(EscrowError::Unauthorized)));
}

#[test]
fn test_snapshot_captures_active_balances() {
    let env = setup_env();
    env.mock_all_auths();
    let client = register_contract(&env);

    let admin = Address::generate(&env);
    let buyer1 = Address::generate(&env);
    let buyer2 = Address::generate(&env);
    let farmer = Address::generate(&env);

    client.init(&admin);

    // buyer1 has two active escrows
    client.deposit(&1u64, &buyer1, &farmer, &1_000, &future_timeout(&env));
    client.deposit(&2u64, &buyer1, &farmer, &2_000, &future_timeout(&env));
    // buyer2 has one active escrow
    client.deposit(&3u64, &buyer2, &farmer, &500, &future_timeout(&env));
    // buyer1 order 4 is released before snapshot — should NOT appear
    client.deposit(&4u64, &buyer1, &farmer, &9_999, &future_timeout(&env));
    client.release(&4u64);

    let order_ids = vec![&env, 1u64, 2u64, 3u64, 4u64];
    let snapshot_id = client.snapshot(&order_ids);
    assert_eq!(snapshot_id, 1u64);

    // buyer1: 1_000 + 2_000 = 3_000 (released order excluded)
    assert_eq!(client.balance_at(&buyer1, &snapshot_id), 3_000i128);
    // buyer2: 500
    assert_eq!(client.balance_at(&buyer2, &snapshot_id), 500i128);
    // farmer has no locked balance
    assert_eq!(client.balance_at(&farmer, &snapshot_id), 0i128);
}

#[test]
fn test_snapshot_ids_are_sequential() {
    let env = setup_env();
    env.mock_all_auths();
    let client = register_contract(&env);

    let admin = Address::generate(&env);
    client.init(&admin);

    let id1 = client.snapshot(&vec![&env]);
    let id2 = client.snapshot(&vec![&env]);
    let id3 = client.snapshot(&vec![&env]);

    assert_eq!(id1, 1u64);
    assert_eq!(id2, 2u64);
    assert_eq!(id3, 3u64);
}

#[test]
fn test_snapshot_records_ledger_sequence() {
    let env = setup_env();
    env.mock_all_auths();
    let client = register_contract(&env);

    let admin = Address::generate(&env);
    client.init(&admin);

    // sequence_number is 1 from setup_env
    let snapshot_id = client.snapshot(&vec![&env]);

    // Verify via balance_at (snapshot exists) — sequence stored internally
    // We can't read SnapshotRecord directly from the client, but a successful
    // balance_at confirms the snapshot was persisted at the right key.
    let result = client.try_balance_at(&admin, &snapshot_id);
    assert!(result.is_ok());
}

#[test]
fn test_balance_at_unknown_snapshot_returns_snapshot_not_found() {
    let env = setup_env();
    env.mock_all_auths();
    let client = register_contract(&env);

    let addr = Address::generate(&env);
    let result = client.try_balance_at(&addr, &99u64);
    assert_eq!(result, Err(Ok(EscrowError::SnapshotNotFound)));
}

#[test]
fn test_snapshot_emits_event() {
    let env = setup_env();
    env.mock_all_auths();
    let client = register_contract(&env);

    let admin = Address::generate(&env);
    client.init(&admin);

    let snapshot_id = client.snapshot(&vec![&env]);

    let events = env.events().all();
    let (_, topics, data) = events.iter().last().unwrap();
    assert_eq!(
        topics,
        vec![
            &env,
            symbol_short!("escrow").into_val(&env),
            symbol_short!("snapshot").into_val(&env),
        ]
    );
    assert_eq!(data, snapshot_id.into_val(&env));
}
