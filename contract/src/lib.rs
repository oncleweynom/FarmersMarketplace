//! Farmers Marketplace — Soroban Escrow Contract
//!
//! Fixes addressed:
//!   #468 — Extend ledger entry TTL so escrow data cannot expire and lock funds.
//!   #469 — Validate buyer != farmer on deposit.
//!   #470 — Validate timeout_unix is at least 1 hour in the future on deposit.
//!   #471 — Emit Soroban events for deposit, release, and refund.

#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, contracterror, symbol_short,
    Address, Env, Map, Vec,
};

// ---------------------------------------------------------------------------
// TTL constants (in ledgers; ~5 s/ledger on Stellar)
//   100 000 ledgers ≈ 5 000 000 s ≈ 57 days  (min)
//   200 000 ledgers ≈ 10 000 000 s ≈ 115 days (max)
// ---------------------------------------------------------------------------
const TTL_MIN: u32 = 100_000;
const TTL_MAX: u32 = 200_000;

/// Minimum timeout duration enforced on deposit (1 hour in seconds).
const MIN_TIMEOUT_SECS: u64 = 3_600;

// ---------------------------------------------------------------------------
// Error enum
// ---------------------------------------------------------------------------
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum EscrowError {
    /// Escrow record already exists for this order_id.
    AlreadyExists = 1,
    /// No escrow record found for this order_id.
    NotFound = 2,
    /// Caller is not authorised to perform this action.
    Unauthorized = 3,
    /// The escrow has not yet timed out.
    NotTimedOut = 4,
    /// The escrow has already been settled (released or refunded).
    AlreadySettled = 5,
    /// buyer and farmer addresses must be different.
    InvalidParties = 6,
    /// Contract has already been initialised.
    AlreadyInitialized = 7,
    /// Snapshot not found for the given snapshot_id.
    SnapshotNotFound = 8,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Escrow record keyed by order_id.
    Escrow(u64),
    /// Admin address (set once via init).
    Admin,
    /// Snapshot record keyed by snapshot_id.
    Snapshot(u64),
    /// Auto-incrementing snapshot sequence counter.
    SnapshotSeq,
}

// ---------------------------------------------------------------------------
// Escrow record stored on-chain
// ---------------------------------------------------------------------------
#[contracttype]
#[derive(Clone)]
pub struct EscrowRecord {
    pub buyer: Address,
    pub farmer: Address,
    pub amount: i128,
    pub timeout_unix: u64,
    pub released: bool,
}

// ---------------------------------------------------------------------------
// Snapshot record stored on-chain
// ---------------------------------------------------------------------------
#[contracttype]
#[derive(Clone)]
pub struct SnapshotRecord {
    /// Ledger sequence number at which the snapshot was taken.
    pub ledger_sequence: u32,
    /// Map of address → escrowed amount at snapshot time.
    pub balances: Map<Address, i128>,
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------
#[contract]
pub struct EscrowContract;

#[contractimpl]
impl EscrowContract {
    // -----------------------------------------------------------------------
    // init
    //
    // Sets the admin address. Must be called once before snapshot() can be used.
    // Subsequent calls return AlreadyInitialized.
    // -----------------------------------------------------------------------
    pub fn init(env: Env, admin: Address) -> Result<(), EscrowError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(EscrowError::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // deposit
    //
    // Locks `amount` tokens in escrow for `order_id`.
    //
    // Validations (fixes #469, #470):
    //   • buyer != farmer
    //   • timeout_unix > now + MIN_TIMEOUT_SECS
    //
    // TTL extension (fix #468):
    //   • Extends the persistent entry TTL after writing.
    //
    // Event emitted (fix #471):
    //   topics : ("escrow", "deposit", order_id)
    //   data   : (buyer, farmer, amount)
    // -----------------------------------------------------------------------
    pub fn deposit(
        env: Env,
        order_id: u64,
        buyer: Address,
        farmer: Address,
        amount: i128,
        timeout_unix: u64,
    ) -> Result<(), EscrowError> {
        // Fix #469 — buyer must differ from farmer
        if buyer == farmer {
            return Err(EscrowError::InvalidParties);
        }

        let key = DataKey::Escrow(order_id);

        // Prevent duplicate deposits for the same order
        if env.storage().persistent().has(&key) {
            return Err(EscrowError::AlreadyExists);
        }

        // Fix #470 — timeout must be at least 1 hour in the future
        let now = env.ledger().timestamp();
        if timeout_unix <= now.saturating_add(MIN_TIMEOUT_SECS) {
            panic!("timeout must be at least 1 hour in the future");
        }

        buyer.require_auth();

        let record = EscrowRecord {
            buyer: buyer.clone(),
            farmer: farmer.clone(),
            amount,
            timeout_unix,
            released: false,
        };

        env.storage().persistent().set(&key, &record);

        // Fix #468 — extend TTL so the entry cannot expire
        env.storage().persistent().extend_ttl(&key, TTL_MIN, TTL_MAX);

        // Fix #471 — emit deposit event
        env.events().publish(
            (symbol_short!("escrow"), symbol_short!("deposit"), order_id),
            (buyer, farmer, amount),
        );

        Ok(())
    }

    // -----------------------------------------------------------------------
    // release
    //
    // Releases escrowed funds to the farmer. Only the buyer may call this.
    //
    // TTL extension (fix #468):
    //   • Extends TTL after updating the record.
    //
    // Event emitted (fix #471):
    //   topics : ("escrow", "release", order_id)
    //   data   : amount
    // -----------------------------------------------------------------------
    pub fn release(env: Env, order_id: u64) -> Result<(), EscrowError> {
        let key = DataKey::Escrow(order_id);

        let mut record: EscrowRecord = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(EscrowError::NotFound)?;

        if record.released {
            return Err(EscrowError::AlreadySettled);
        }

        // Only the buyer can release funds to the farmer
        record.buyer.require_auth();

        let amount = record.amount;
        record.released = true;

        env.storage().persistent().set(&key, &record);

        // Fix #468 — extend TTL after update
        env.storage().persistent().extend_ttl(&key, TTL_MIN, TTL_MAX);

        // Fix #471 — emit release event
        env.events().publish(
            (symbol_short!("escrow"), symbol_short!("release"), order_id),
            amount,
        );

        Ok(())
    }

    // -----------------------------------------------------------------------
    // refund
    //
    // Returns escrowed funds to the buyer after the timeout has passed.
    // Anyone may call this once the timeout is reached (permissionless sweep).
    //
    // TTL extension (fix #468):
    //   • Extends TTL after updating the record.
    //
    // Event emitted (fix #471):
    //   topics : ("escrow", "refund", order_id)
    //   data   : amount
    // -----------------------------------------------------------------------
    pub fn refund(env: Env, order_id: u64) -> Result<(), EscrowError> {
        let key = DataKey::Escrow(order_id);

        let mut record: EscrowRecord = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(EscrowError::NotFound)?;

        if record.released {
            return Err(EscrowError::AlreadySettled);
        }

        let now = env.ledger().timestamp();
        if now < record.timeout_unix {
            return Err(EscrowError::NotTimedOut);
        }

        let amount = record.amount;
        record.released = true;

        env.storage().persistent().set(&key, &record);

        // Fix #468 — extend TTL after update
        env.storage().persistent().extend_ttl(&key, TTL_MIN, TTL_MAX);

        // Fix #471 — emit refund event
        env.events().publish(
            (symbol_short!("escrow"), symbol_short!("refund"), order_id),
            amount,
        );

        Ok(())
    }

    // -----------------------------------------------------------------------
    // get_escrow — read-only helper
    // -----------------------------------------------------------------------
    pub fn get_escrow(env: Env, order_id: u64) -> Result<EscrowRecord, EscrowError> {
        let key = DataKey::Escrow(order_id);
        env.storage()
            .persistent()
            .get(&key)
            .ok_or(EscrowError::NotFound)
    }

    // -----------------------------------------------------------------------
    // snapshot
    //
    // Admin-only. Iterates all active (unreleased) escrow records by scanning
    // the provided order_ids list and records each buyer's locked balance at
    // the current ledger sequence. Returns the new snapshot_id.
    //
    // Parameters:
    //   order_ids — the list of order IDs to include in the snapshot.
    //               Callers supply this because Soroban contracts cannot
    //               enumerate storage keys on-chain.
    // -----------------------------------------------------------------------
    pub fn snapshot(env: Env, order_ids: Vec<u64>) -> Result<u64, EscrowError> {
        // Only admin may call this
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(EscrowError::Unauthorized)?;
        admin.require_auth();

        // Assign the next snapshot_id
        let snapshot_id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::SnapshotSeq)
            .unwrap_or(0u64)
            + 1;
        env.storage()
            .instance()
            .set(&DataKey::SnapshotSeq, &snapshot_id);

        // Build address → total locked balance map
        let mut balances: Map<Address, i128> = Map::new(&env);

        for order_id in order_ids.iter() {
            let key = DataKey::Escrow(order_id);
            if let Some(record) = env
                .storage()
                .persistent()
                .get::<DataKey, EscrowRecord>(&key)
            {
                if !record.released {
                    let prev: i128 = balances.get(record.buyer.clone()).unwrap_or(0);
                    balances.set(record.buyer.clone(), prev + record.amount);
                }
            }
        }

        let snap = SnapshotRecord {
            ledger_sequence: env.ledger().sequence(),
            balances,
        };

        let snap_key = DataKey::Snapshot(snapshot_id);
        env.storage().persistent().set(&snap_key, &snap);
        env.storage()
            .persistent()
            .extend_ttl(&snap_key, TTL_MIN, TTL_MAX);

        env.events().publish(
            (symbol_short!("escrow"), symbol_short!("snapshot")),
            snapshot_id,
        );

        Ok(snapshot_id)
    }

    // -----------------------------------------------------------------------
    // balance_at
    //
    // Read-only. Returns the locked balance of `addr` recorded in `snapshot_id`.
    // Returns 0 if the address had no locked funds at that snapshot.
    // -----------------------------------------------------------------------
    pub fn balance_at(
        env: Env,
        addr: Address,
        snapshot_id: u64,
    ) -> Result<i128, EscrowError> {
        let snap_key = DataKey::Snapshot(snapshot_id);
        let snap: SnapshotRecord = env
            .storage()
            .persistent()
            .get(&snap_key)
            .ok_or(EscrowError::SnapshotNotFound)?;

        Ok(snap.balances.get(addr).unwrap_or(0))
    }
}

mod test;
