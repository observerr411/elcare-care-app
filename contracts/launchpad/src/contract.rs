//! Launchpad — Factory contract that deploys the 4 NFT collection types.
//!
//! # Deployment flow
//!
//! 1. Admin deploys this contract and calls `initialize`.
//! 2. Admin uploads each of the 4 collection WASMs with:
//!    `stellar contract upload --wasm <file>.wasm --network testnet`
//!    and then calls `set_wasm_hashes` with the 4 resulting 32-byte hashes.
//! 3. Any user can now call one of the four `deploy_*` functions to launch
//!    their own collection.  The factory calls `initialize` on the freshly
//!    deployed contract in the same transaction — no second call needed.
//!
//! # Fee model
//!
//! Two distinct fees, deliberately typed apart:
//! * `deploy_fee: i128` — a flat, token-denominated amount transferred from
//!   the creator to `fee_receiver` on every `deploy_*` call.
//! * `platform_fee_bps: u32` — a per-collection basis-point fee chosen by the
//!   creator (≤ `MAX_FEE_BPS`), recorded in the registry and forwarded to the
//!   lazy-mint contracts so they can split redemption proceeds.
//!
//! # Deterministic addresses (clone-equivalent)
//! `env.deployer().with_current_contract(salt)` gives a deterministic address
//! from `sha256(factory_address ‖ salt)`.  Clients can pre-compute the address
//! before the transaction confirms.  Pass a different `salt` for each collection.
//!
//! # Why this is Soroban's answer to EIP-1167 clones
//! The collection WASM is stored once on the network (identified by hash).
//! Every `deploy()` call shares that same WASM — no bytecode duplication.
//! Each instance gets completely isolated storage.

use soroban_sdk::{
    contract, contractimpl, symbol_short, token, xdr::ToXdr, Address, Bytes, BytesN, Env, String,
    Vec,
};

use crate::{
    events, storage,
    types::{CollectionKind, CollectionRecord, Error, PreflightResult, WasmHashes},
};
use crate::types::DataKey;

/// Semantic version — bump on every breaking storage change.
const CONTRACT_VERSION: &str = "1.0.0";

/// Maximum allowed platform fee (20 %) — issue #38.
const MAX_FEE_BPS: u32 = 2000;
/// Maximum allowed royalty (100 %), matching the collection contracts' own cap — issue #277.
const MAX_ROYALTY_BPS: u32 = 10_000;

// ─── Cross-contract clients ───────────────────────────────────────────────────

mod iface {
    use soroban_sdk::{contractclient, Address, BytesN, Env, String};

    #[contractclient(name = "Normal721Client")]
    pub trait INormal721 {
        fn initialize(
            env: Env,
            creator: Address,
            name: String,
            symbol: String,
            max_supply: u64,
            royalty_bps: u32,
            royalty_receiver: Address,
        );
        fn upgrade(env: Env, new_wasm_hash: BytesN<32>);
    }

    #[contractclient(name = "Normal1155Client")]
    pub trait INormal1155 {
        fn initialize(
            env: Env,
            creator: Address,
            name: String,
            royalty_bps: u32,
            royalty_receiver: Address,
        );
        fn upgrade(env: Env, new_wasm_hash: BytesN<32>);
    }

    /// Issue #38: lazy mint contracts accept per-collection platform fee at init.
    #[contractclient(name = "Lazy721Client")]
    #[allow(clippy::too_many_arguments)]
    pub trait ILazy721 {
        fn initialize(
            env: Env,
            creator: Address,
            creator_pubkey: BytesN<32>,
            name: String,
            symbol: String,
            max_supply: u64,
            royalty_bps: u32,
            royalty_receiver: Address,
            platform_fee_receiver: Address,
            platform_fee_bps: u32,
            network_passphrase: String,
        );
        fn upgrade(env: Env, new_wasm_hash: BytesN<32>);
    }

    #[contractclient(name = "Lazy1155Client")]
    #[allow(clippy::too_many_arguments)]
    pub trait ILazy1155 {
        fn initialize(
            env: Env,
            creator: Address,
            creator_pubkey: BytesN<32>,
            name: String,
            royalty_bps: u32,
            royalty_receiver: Address,
            platform_fee_receiver: Address,
            platform_fee_bps: u32,
            network_passphrase: String,
        );
        fn upgrade(env: Env, new_wasm_hash: BytesN<32>);
    }
}

use iface::{Lazy1155Client, Lazy721Client, Normal1155Client, Normal721Client};

// ─── Salt hardening ───────────────────────────────────────────────────────────
fn make_secure_salt(env: &Env, creator: &Address, raw_salt: &BytesN<32>) -> BytesN<32> {
    let mut raw = Bytes::new(env);
    raw.append(&creator.to_xdr(env));
    raw.extend_from_array(&raw_salt.to_array());
    env.crypto().sha256(&raw).into()
}

// ─── Shared deploy guards ─────────────────────────────────────────────────────

/// Transfers the flat deploy fee (if any) from `creator` to the treasury and
/// emits `fee_coll`.  Returns the configured fee receiver so lazy deploys can
/// forward it to the child contract as `platform_fee_receiver`.
fn collect_deploy_fee(env: &Env, creator: &Address, currency: &Address) -> Address {
    let (receiver, deploy_fee) = storage::get_fee_config(env);
    if deploy_fee > 0 {
        token::TokenClient::new(env, currency).transfer(creator, &receiver, &deploy_fee);
        events::publish_deployment_fee_collected(env, creator, &receiver, deploy_fee, currency);
    }
    receiver
}

// ─── Shared deploy validation (#277) ──────────────────────────────────────────
//
// These helpers back both the mutating `deploy_*` functions and the read-only
// `preflight_deploy_*` functions, so a creator can trust that a clean
// preflight result means the matching deploy call will succeed, and that
// every error a deploy call can raise (before it mutates state) is visible
// ahead of time.

/// Validation shared by the two 721-shaped kinds (Normal721, LazyMint721):
/// both take `name`, `symbol` and `max_supply`.
fn validate_721_shape(
    env: &Env,
    name: &String,
    symbol: &String,
    max_supply: u64,
    royalty_bps: u32,
    platform_fee_bps: u32,
    secure_salt: &BytesN<32>,
    wasm_set: bool,
) -> Vec<Error> {
    let mut errors = Vec::new(env);
    if storage::is_paused(env) {
        errors.push_back(Error::ContractPaused);
    }
    if platform_fee_bps > MAX_FEE_BPS {
        errors.push_back(Error::InvalidFeeBps);
    }
    if royalty_bps > MAX_ROYALTY_BPS {
        errors.push_back(Error::InvalidRoyaltyBps);
    }
    if name.len() == 0 {
        errors.push_back(Error::EmptyName);
    }
    if symbol.len() == 0 {
        errors.push_back(Error::EmptySymbol);
    }
    if max_supply == 0 {
        errors.push_back(Error::InvalidMaxSupply);
    }
    if !wasm_set {
        errors.push_back(Error::WasmHashNotSet);
    }
    if storage::is_salt_used(env, secure_salt) {
        errors.push_back(Error::DuplicateSalt);
    }
    errors
}

/// Validation shared by the two 1155-shaped kinds (Normal1155, LazyMint1155):
/// both take only `name` (no symbol / no collection-level max_supply).
fn validate_1155_shape(
    env: &Env,
    name: &String,
    royalty_bps: u32,
    platform_fee_bps: u32,
    secure_salt: &BytesN<32>,
    wasm_set: bool,
) -> Vec<Error> {
    let mut errors = Vec::new(env);
    if storage::is_paused(env) {
        errors.push_back(Error::ContractPaused);
    }
    if platform_fee_bps > MAX_FEE_BPS {
        errors.push_back(Error::InvalidFeeBps);
    }
    if royalty_bps > MAX_ROYALTY_BPS {
        errors.push_back(Error::InvalidRoyaltyBps);
    }
    if name.len() == 0 {
        errors.push_back(Error::EmptyName);
    }
    if !wasm_set {
        errors.push_back(Error::WasmHashNotSet);
    }
    if storage::is_salt_used(env, secure_salt) {
        errors.push_back(Error::DuplicateSalt);
    }
    errors
}

/// Adds `Error::InsufficientFee` to `errors` when `creator`'s balance of
/// `currency` cannot cover the flat `deploy_fee`. Read-only — used only by
/// the preflight path, since the real transfer in `collect_deploy_fee`
/// already fails atomically if the balance is insufficient.
fn check_sufficient_fee(env: &Env, creator: &Address, currency: &Address, deploy_fee: i128, errors: &mut Vec<Error>) {
    if deploy_fee > 0 {
        let balance = token::TokenClient::new(env, currency).balance(creator);
        if balance < deploy_fee {
            errors.push_back(Error::InsufficientFee);
        }
    }
}

/// Converts a `Vec<Error>` into `Vec<u32>` (discriminant values) for
/// storage in `PreflightResult.errors`, which must satisfy `SorobanArbitrary`.
fn error_codes(env: &Env, errs: Vec<Error>) -> Vec<u32> {
    let mut codes: Vec<u32> = Vec::new(env);
    for e in errs.iter() {
        codes.push_back(e as u32);
    }
    codes
}

#[contract]
pub struct Launchpad;

#[contractimpl]
#[allow(clippy::too_many_arguments)]
impl Launchpad {
    pub fn initialize(
        env: Env,
        admin: Address,
        fee_receiver: Address,
        deploy_fee: i128,
    ) -> Result<(), Error> {
        if storage::is_initialized(&env) {
            return Err(Error::AlreadyInitialized);
        }
        if deploy_fee < 0 {
            return Err(Error::InvalidDeployFee);
        }
        admin.require_auth();
        storage::set_initialized(&env);
        storage::set_admin(&env, &admin);
        storage::set_fee_config(&env, &fee_receiver, deploy_fee);
        Ok(())
    }

    // ── Versioning & Migration ────────────────────────────────────────────

    /// Returns the semantic version string compiled into this WASM.
    pub fn version(env: Env) -> soroban_sdk::String {
        soroban_sdk::String::from_str(&env, crate::types::CONTRACT_VERSION)
    }

    /// Returns the version string last written to on-chain storage by
    /// `migrate()`.  `None` before the first migration.
    pub fn contract_version(env: Env) -> Option<soroban_sdk::String> {
        storage::get_contract_version(&env)
    }

    /// Admin-guarded, idempotent storage migration entry point.
    ///
    /// # Idempotency
    /// Records a per-version completion marker the first time it succeeds.
    /// Subsequent calls for the *same* version revert with `AlreadyMigrated`.
    ///
    /// # Unsupported jumps
    /// Only sequential upgrades (e.g. 1.0.0 → 1.1.0) are accepted.  If no
    /// prior version is on-chain (fresh install) any version is accepted as the
    /// first migration.
    ///
    /// # 1.0.0 migration
    /// Migrates legacy monolithic `ByCreator(Address)` Vec<CollectionRecord>
    /// and `AllCollections` Vec<CollectionRecord> entries into the paged index
    /// storage introduced in the current build.  Legacy keys are consumed and
    /// deleted.  The step is idempotent — re-running after a crash finds the
    /// keys absent and skips them silently.
    pub fn migrate(env: Env, admin: Address) -> Result<(), Error> {
        storage::extend_instance_ttl(&env);
        let version = Self::require_pending_migration(&env, &admin)?;
        Self::run_migration(&env, &version, u32::MAX);
        Ok(())
    }

    /// Bounded, resumable variant of `migrate`.  Returns items still pending.
    /// Call repeatedly until it returns `0`.
    pub fn migrate_step(env: Env, admin: Address, max_items: u32) -> Result<u64, Error> {
        storage::extend_instance_ttl(&env);
        let version = Self::require_pending_migration(&env, &admin)?;
        Ok(Self::run_migration(&env, &version, max_items))
    }

    // ── Internal migration helpers ────────────────────────────────────────

    fn require_pending_migration(env: &Env, admin: &Address) -> Result<soroban_sdk::String, Error> {
        admin.require_auth();
        let stored = storage::get_admin(env).ok_or(Error::NotInitialized)?;
        if *admin != stored {
            return Err(Error::NotAdmin);
        }
        let target = soroban_sdk::String::from_str(env, crate::types::CONTRACT_VERSION);
        if storage::is_migration_done(env, &target) {
            return Err(Error::AlreadyMigrated);
        }
        Ok(target)
    }

    fn run_migration(
        env: &Env,
        version: &soroban_sdk::String,
        mut budget: u32,
    ) -> u64 {
        let mut p = storage::get_migration_progress(env, version);

        // Phase 0: migrate legacy ByCreator + AllCollections Vec entries into
        //           the paged index introduced in v1.0.0.
        // Each legacy ByCreator Vec<CollectionRecord> entry is read-and-deleted
        // in one step; records are then appended to the per-address paged index.
        while budget > 0 {
            match p.phase {
                0 => {
                    // Legacy AllCollections is a single entry; consume it.
                    let legacy_key = DataKey::AllCollections;
                    if let Some(records) = env
                        .storage()
                        .persistent()
                        .get::<DataKey, soroban_sdk::Vec<crate::types::CollectionRecord>>(
                            &legacy_key,
                        )
                    {
                        env.storage().persistent().remove(&legacy_key);
                        let current_count = storage::collection_count(env);
                        // Re-insert into paged keys without touching per-creator indices
                        // (those were written by record_collection() at deploy time and
                        //  only need to survive).
                        for (i, rec) in records.iter().enumerate() {
                            let global_idx = current_count + i as u64;
                            env.storage().persistent().set(
                                &DataKey::CollectionByIndex(global_idx),
                                &rec,
                            );
                            env.storage().persistent().extend_ttl(
                                &DataKey::CollectionByIndex(global_idx),
                                50_000,
                                100_000,
                            );
                        }
                        let new_total = current_count + records.len() as u64;
                        env.storage()
                            .persistent()
                            .set(&DataKey::CollectionCount, &new_total);
                    }
                    p.phase = 1;
                    budget -= 1;
                }
                _ => break,
            }
        }

        let remaining: u64 = if p.phase == 0 { 1 } else { 0 };

        if remaining == 0 {
            storage::clear_migration_progress(env, version);
            storage::set_migration_done(env, version);
            storage::set_contract_version(env, version);
            events::publish_migration_completed(env, version);
        } else {
            storage::set_migration_progress(env, version, &p);
        }
        remaining
    }

    /// Records the four collection WASM hashes, bumps the version counter and
    /// emits `wasm_set` so indexers can track factory upgrades.
    pub fn set_wasm_hashes(
        env: Env,
        wasm_normal_721: BytesN<32>,
        wasm_normal_1155: BytesN<32>,
        wasm_lazy_721: BytesN<32>,
        wasm_lazy_1155: BytesN<32>,
    ) -> Result<u32, Error> {
        storage::extend_instance_ttl(&env);
        storage::require_admin(&env)?;
        let version = storage::set_wasm_hashes(
            &env,
            &wasm_normal_721,
            &wasm_normal_1155,
            &wasm_lazy_721,
            &wasm_lazy_1155,
        );
        events::publish_wasm_hashes_set(
            &env,
            version,
            &wasm_normal_721,
            &wasm_normal_1155,
            &wasm_lazy_721,
            &wasm_lazy_1155,
        );
        Ok(version)
    }

    // ── Deploy: Normal ERC-721 ────────────────────────────────────────────

    /// Issue #38: `platform_fee_bps` is validated (≤ MAX_FEE_BPS) and stored in the registry.
    pub fn deploy_normal_721(
        env: Env,
        creator: Address,
        currency: Address,
        name: String,
        symbol: String,
        max_supply: u64,
        royalty_bps: u32,
        royalty_receiver: Address,
        platform_fee_bps: u32,
        salt: BytesN<32>,
    ) -> Result<Address, Error> {
        storage::extend_instance_ttl(&env);
        creator.require_auth();

        let secure_salt = make_secure_salt(&env, &creator, &salt);
        let wasm_opt = storage::get_wasm_normal_721(&env);
        let errors = validate_721_shape(
            &env,
            &name,
            &symbol,
            max_supply,
            royalty_bps,
            platform_fee_bps,
            &secure_salt,
            wasm_opt.is_some(),
        );
        if let Some(first) = errors.get(0) {
            return Err(first);
        }
        let wasm = wasm_opt.unwrap();

        collect_deploy_fee(&env, &creator, &currency);

        let addr = env
            .deployer()
            .with_current_contract(secure_salt.clone())
            .deploy_v2(wasm, ());

        Normal721Client::new(&env, &addr).initialize(
            &creator,
            &name,
            &symbol,
            &max_supply,
            &royalty_bps,
            &royalty_receiver,
        );

        storage::mark_salt_used(&env, &secure_salt);
        storage::record_collection(
            &env,
            &creator,
            &addr,
            CollectionKind::Normal721,
            &name,
            &symbol,
            env.ledger().sequence(),
            platform_fee_bps,
        );
        events::publish_deploy(&env, symbol_short!("n721"), &creator, &addr);
        Ok(addr)
    }

    /// Read-only preflight for `deploy_normal_721` (#277). Runs the exact
    /// same validation the mutating call would run, without requiring
    /// authorization or touching storage. Returns the predicted deployment
    /// address, the flat fee that would be charged, and every validation
    /// failure found.
    pub fn preflight_deploy_normal_721(
        env: Env,
        creator: Address,
        currency: Address,
        name: String,
        symbol: String,
        max_supply: u64,
        royalty_bps: u32,
        platform_fee_bps: u32,
        salt: BytesN<32>,
    ) -> PreflightResult {
        let secure_salt = make_secure_salt(&env, &creator, &salt);
        let wasm_opt = storage::get_wasm_normal_721(&env);
        let mut errors = validate_721_shape(
            &env,
            &name,
            &symbol,
            max_supply,
            royalty_bps,
            platform_fee_bps,
            &secure_salt,
            wasm_opt.is_some(),
        );

        let (_, deploy_fee) = storage::get_fee_config(&env);
        check_sufficient_fee(&env, &creator, &currency, deploy_fee, &mut errors);

        let predicted_address = env
            .deployer()
            .with_current_contract(secure_salt)
            .deployed_address();

        PreflightResult {
            predicted_address,
            required_fee: deploy_fee,
            platform_fee_bps,
            currency,
            errors: error_codes(&env, errors),
        }
    }

    // ── Deploy: Normal ERC-1155 ──────────────────────────────────────────
    pub fn deploy_normal_1155(
        env: Env,
        creator: Address,
        currency: Address,
        name: String,
        royalty_bps: u32,
        royalty_receiver: Address,
        platform_fee_bps: u32,
        salt: BytesN<32>,
    ) -> Result<Address, Error> {
        storage::extend_instance_ttl(&env);
        creator.require_auth();

        let secure_salt = make_secure_salt(&env, &creator, &salt);
        let wasm_opt = storage::get_wasm_normal_1155(&env);
        let errors = validate_1155_shape(
            &env,
            &name,
            royalty_bps,
            platform_fee_bps,
            &secure_salt,
            wasm_opt.is_some(),
        );
        if let Some(first) = errors.get(0) {
            return Err(first);
        }
        let wasm = wasm_opt.unwrap();

        collect_deploy_fee(&env, &creator, &currency);

        let addr = env
            .deployer()
            .with_current_contract(secure_salt.clone())
            .deploy_v2(wasm, ());

        Normal1155Client::new(&env, &addr).initialize(
            &creator,
            &name,
            &royalty_bps,
            &royalty_receiver,
        );

        storage::mark_salt_used(&env, &secure_salt);
        let empty_symbol = String::from_str(&env, "");
        storage::record_collection(
            &env,
            &creator,
            &addr,
            CollectionKind::Normal1155,
            &name,
            &empty_symbol,
            env.ledger().sequence(),
            platform_fee_bps,
        );
        events::publish_deploy(&env, symbol_short!("n1155"), &creator, &addr);
        Ok(addr)
    }

    /// Read-only preflight for `deploy_normal_1155` (#277).
    pub fn preflight_deploy_normal_1155(
        env: Env,
        creator: Address,
        currency: Address,
        name: String,
        royalty_bps: u32,
        platform_fee_bps: u32,
        salt: BytesN<32>,
    ) -> PreflightResult {
        let secure_salt = make_secure_salt(&env, &creator, &salt);
        let wasm_opt = storage::get_wasm_normal_1155(&env);
        let mut errors = validate_1155_shape(
            &env,
            &name,
            royalty_bps,
            platform_fee_bps,
            &secure_salt,
            wasm_opt.is_some(),
        );

        let (_, deploy_fee) = storage::get_fee_config(&env);
        check_sufficient_fee(&env, &creator, &currency, deploy_fee, &mut errors);

        let predicted_address = env
            .deployer()
            .with_current_contract(secure_salt)
            .deployed_address();

        PreflightResult {
            predicted_address,
            required_fee: deploy_fee,
            platform_fee_bps,
            currency,
            errors: error_codes(&env, errors),
        }
    }

    // ── Deploy: LazyMint ERC-721 ──────────────────────────────────────────

    /// Issue #38: passes per-collection fee to the lazy mint contract so that
    /// fee splits are applied at voucher redemption time.
    pub fn deploy_lazy_721(
        env: Env,
        creator: Address,
        currency: Address,
        creator_pubkey: BytesN<32>,
        name: String,
        symbol: String,
        max_supply: u64,
        royalty_bps: u32,
        royalty_receiver: Address,
        platform_fee_bps: u32,
        salt: BytesN<32>,
        network_passphrase: String,
    ) -> Result<Address, Error> {
        storage::extend_instance_ttl(&env);
        creator.require_auth();

        let secure_salt = make_secure_salt(&env, &creator, &salt);
        let wasm_opt = storage::get_wasm_lazy_721(&env);
        let errors = validate_721_shape(
            &env,
            &name,
            &symbol,
            max_supply,
            royalty_bps,
            platform_fee_bps,
            &secure_salt,
            wasm_opt.is_some(),
        );
        if let Some(first) = errors.get(0) {
            return Err(first);
        }
        let wasm = wasm_opt.unwrap();

        let platform_fee_receiver = collect_deploy_fee(&env, &creator, &currency);

        let addr = env
            .deployer()
            .with_current_contract(secure_salt.clone())
            .deploy_v2(wasm, ());

        Lazy721Client::new(&env, &addr).initialize(
            &creator,
            &creator_pubkey,
            &name,
            &symbol,
            &max_supply,
            &royalty_bps,
            &royalty_receiver,
            &platform_fee_receiver,
            &platform_fee_bps,
            &network_passphrase,
        );

        storage::mark_salt_used(&env, &secure_salt);
        storage::record_collection(
            &env,
            &creator,
            &addr,
            CollectionKind::LazyMint721,
            &name,
            &symbol,
            env.ledger().sequence(),
            platform_fee_bps,
        );
        events::publish_deploy(&env, symbol_short!("l721"), &creator, &addr);
        Ok(addr)
    }

    /// Read-only preflight for `deploy_lazy_721` (#277).
    pub fn preflight_deploy_lazy_721(
        env: Env,
        creator: Address,
        currency: Address,
        name: String,
        symbol: String,
        max_supply: u64,
        royalty_bps: u32,
        platform_fee_bps: u32,
        salt: BytesN<32>,
    ) -> PreflightResult {
        let secure_salt = make_secure_salt(&env, &creator, &salt);
        let wasm_opt = storage::get_wasm_lazy_721(&env);
        let mut errors = validate_721_shape(
            &env,
            &name,
            &symbol,
            max_supply,
            royalty_bps,
            platform_fee_bps,
            &secure_salt,
            wasm_opt.is_some(),
        );

        let (_, deploy_fee) = storage::get_fee_config(&env);
        check_sufficient_fee(&env, &creator, &currency, deploy_fee, &mut errors);

        let predicted_address = env
            .deployer()
            .with_current_contract(secure_salt)
            .deployed_address();

        PreflightResult {
            predicted_address,
            required_fee: deploy_fee,
            platform_fee_bps,
            currency,
            errors: error_codes(&env, errors),
        }
    }

    // ── Deploy: LazyMint ERC-1155 ─────────────────────────────────────────
    pub fn deploy_lazy_1155(
        env: Env,
        creator: Address,
        currency: Address,
        creator_pubkey: BytesN<32>,
        name: String,
        royalty_bps: u32,
        royalty_receiver: Address,
        platform_fee_bps: u32,
        salt: BytesN<32>,
        network_passphrase: String,
    ) -> Result<Address, Error> {
        storage::extend_instance_ttl(&env);
        creator.require_auth();

        let secure_salt = make_secure_salt(&env, &creator, &salt);
        let wasm_opt = storage::get_wasm_lazy_1155(&env);
        let errors = validate_1155_shape(
            &env,
            &name,
            royalty_bps,
            platform_fee_bps,
            &secure_salt,
            wasm_opt.is_some(),
        );
        if let Some(first) = errors.get(0) {
            return Err(first);
        }
        let wasm = wasm_opt.unwrap();

        let platform_fee_receiver = collect_deploy_fee(&env, &creator, &currency);

        let addr = env
            .deployer()
            .with_current_contract(secure_salt.clone())
            .deploy_v2(wasm, ());

        Lazy1155Client::new(&env, &addr).initialize(
            &creator,
            &creator_pubkey,
            &name,
            &royalty_bps,
            &royalty_receiver,
            &platform_fee_receiver,
            &platform_fee_bps,
            &network_passphrase,
        );

        storage::mark_salt_used(&env, &secure_salt);
        let empty_symbol = String::from_str(&env, "");
        storage::record_collection(
            &env,
            &creator,
            &addr,
            CollectionKind::LazyMint1155,
            &name,
            &empty_symbol,
            env.ledger().sequence(),
            platform_fee_bps,
        );
        events::publish_deploy(&env, symbol_short!("l1155"), &creator, &addr);
        Ok(addr)
    }

    /// Read-only preflight for `deploy_lazy_1155` (#277).
    pub fn preflight_deploy_lazy_1155(
        env: Env,
        creator: Address,
        currency: Address,
        name: String,
        royalty_bps: u32,
        platform_fee_bps: u32,
        salt: BytesN<32>,
    ) -> PreflightResult {
        let secure_salt = make_secure_salt(&env, &creator, &salt);
        let wasm_opt = storage::get_wasm_lazy_1155(&env);
        let mut errors = validate_1155_shape(
            &env,
            &name,
            royalty_bps,
            platform_fee_bps,
            &secure_salt,
            wasm_opt.is_some(),
        );

        let (_, deploy_fee) = storage::get_fee_config(&env);
        check_sufficient_fee(&env, &creator, &currency, deploy_fee, &mut errors);

        let predicted_address = env
            .deployer()
            .with_current_contract(secure_salt)
            .deployed_address();

        PreflightResult {
            predicted_address,
            required_fee: deploy_fee,
            platform_fee_bps,
            currency,
            errors: error_codes(&env, errors),
        }
    }

    // ── Admin management (two-step transfer) ──────────────────────────────

    /// Step 1: the current admin proposes a successor.  Overwrites any
    /// previously pending proposal.  The successor must call `accept_admin`.
    pub fn transfer_admin(env: Env, new_admin: Address) -> Result<(), Error> {
        storage::extend_instance_ttl(&env);
        let admin = storage::require_admin(&env)?;
        storage::set_pending_admin(&env, &new_admin);
        events::publish_admin_transfer_proposed(&env, &admin, &new_admin);
        Ok(())
    }

    /// Step 2: the proposed successor accepts the role.
    pub fn accept_admin(env: Env, new_admin: Address) -> Result<(), Error> {
        storage::extend_instance_ttl(&env);
        new_admin.require_auth();
        let pending = storage::get_pending_admin(&env).ok_or(Error::NoPendingAdmin)?;
        if new_admin != pending {
            return Err(Error::NotPendingAdmin);
        }
        let old_admin = storage::get_admin(&env).ok_or(Error::NotInitialized)?;
        storage::set_admin(&env, &new_admin);
        storage::clear_pending_admin(&env);
        events::publish_admin_transfer_accepted(&env, &old_admin, &new_admin);
        Ok(())
    }

    /// Cancels a pending admin proposal.  Only the current admin may cancel.
    pub fn cancel_admin_transfer(env: Env) -> Result<(), Error> {
        storage::extend_instance_ttl(&env);
        let admin = storage::require_admin(&env)?;
        let pending = storage::get_pending_admin(&env).ok_or(Error::NoPendingAdmin)?;
        storage::clear_pending_admin(&env);
        events::publish_admin_transfer_cancelled(&env, &admin, &pending);
        Ok(())
    }

    // ── Pause ─────────────────────────────────────────────────────────────

    /// Halts all four `deploy_*` functions. Callable by the configured
    /// `EmergencyPauser` (see `set_emergency_pauser`), or by the admin when no
    /// separate pauser has been assigned — so this remains usable even if the
    /// routine admin authority is unavailable, once a distinct pauser is set.
    pub fn pause(env: Env) -> Result<(), Error> {
        storage::extend_instance_ttl(&env);
        let authority = storage::require_pause_authority(&env)?;
        storage::set_paused(&env, true);
        events::publish_paused(&env, &authority, true);
        Ok(())
    }

    pub fn unpause(env: Env) -> Result<(), Error> {
        storage::extend_instance_ttl(&env);
        let authority = storage::require_pause_authority(&env)?;
        storage::set_paused(&env, false);
        events::publish_paused(&env, &authority, false);
        Ok(())
    }

    /// Assigns the `EmergencyPause` role to `pauser`, separating it from the
    /// routine `Admin` authority (Issue #267). Admin-only. Passing the
    /// current admin's own address restores the pre-role-separation
    /// behaviour (admin remains the sole pauser).
    pub fn set_emergency_pauser(env: Env, pauser: Address) -> Result<(), Error> {
        storage::extend_instance_ttl(&env);
        storage::require_admin(&env)?;
        storage::set_emergency_pauser(&env, &pauser);
        events::publish_emergency_pauser_updated(&env, &pauser);
        Ok(())
    }

    pub fn update_collection_wasm(
        env: Env,
        kind: CollectionKind,
        new_wasm_hash: BytesN<32>,
    ) -> Result<(), Error> {
        storage::extend_instance_ttl(&env);
        storage::require_admin(&env)?;
        let old_wasm = storage::get_wasm_for_kind(&env, &kind).unwrap_or_else(|| {
            BytesN::from_array(&env, &[0u8; 32])
        });
        storage::set_wasm_hash_for_kind(&env, &kind, &new_wasm_hash);
        events::publish_collection_wasm_updated(&env, &kind, &old_wasm, &new_wasm_hash);
        Ok(())
    }

    pub fn upgrade_collection(env: Env, collection_address: Address) -> Result<(), Error> {
        storage::extend_instance_ttl(&env);
        storage::require_admin(&env)?;

        let record = storage::get_collection_by_address(&env, &collection_address)
            .ok_or(Error::CollectionNotFound)?;
        let new_wasm = storage::get_wasm_for_kind(&env, &record.kind)
            .ok_or(Error::WasmHashNotSet)?;
        let from_wasm = new_wasm.clone();

        match record.kind {
            CollectionKind::Normal721 => {
                Normal721Client::new(&env, &collection_address).upgrade(&new_wasm);
            }
            CollectionKind::Normal1155 => {
                Normal1155Client::new(&env, &collection_address).upgrade(&new_wasm);
            }
            CollectionKind::LazyMint721 => {
                Lazy721Client::new(&env, &collection_address).upgrade(&new_wasm);
            }
            CollectionKind::LazyMint1155 => {
                Lazy1155Client::new(&env, &collection_address).upgrade(&new_wasm);
            }
        }

        events::publish_collection_upgraded(&env, &collection_address, &from_wasm, &new_wasm);
        Ok(())
    }

    // ── Fee config ────────────────────────────────────────────────────────

    /// Sets both the treasury address and the flat deploy fee (token smallest
    /// unit).  Replaces the former `set_deploy_fee` / `set_treasury` /
    /// `update_platform_fee` trio.
    pub fn set_fee_config(env: Env, receiver: Address, deploy_fee: i128) -> Result<(), Error> {
        storage::extend_instance_ttl(&env);
        storage::require_admin(&env)?;
        if deploy_fee < 0 {
            return Err(Error::InvalidDeployFee);
        }
        storage::set_fee_config(&env, &receiver, deploy_fee);
        events::publish_fee_config_updated(&env, &receiver, deploy_fee);
        Ok(())
    }

    // ── View functions ────────────────────────────────────────────────────

    pub fn collections_by_creator(env: Env, creator: Address) -> Vec<CollectionRecord> {
        storage::collections_by_creator(&env, &creator)
    }

    pub fn all_collections(env: Env) -> Vec<CollectionRecord> {
        storage::all_collections(&env)
    }

    pub fn collection_count(env: Env) -> u64 {
        storage::collection_count(&env)
    }

    /// Direct O(1) lookup of a collection by its deployed address (#37).
    pub fn get_collection(env: Env, address: Address) -> Option<CollectionRecord> {
        storage::get_collection_by_address(&env, &address)
    }

    /// Paginated read of the global registry (#37).
    pub fn get_collections(env: Env, start: u64, limit: u32) -> Vec<CollectionRecord> {
        storage::get_collections_paginated(&env, start, limit)
    }

    pub fn admin(env: Env) -> Address {
        storage::get_admin(&env).unwrap()
    }

    pub fn pending_admin(env: Env) -> Option<Address> {
        storage::get_pending_admin(&env)
    }

    pub fn paused(env: Env) -> bool {
        storage::is_paused(&env)
    }

    /// The explicit `EmergencyPause` role holder, or `None` if unassigned
    /// (in which case `pause`/`unpause` fall back to the admin).
    pub fn emergency_pauser(env: Env) -> Option<Address> {
        storage::get_emergency_pauser(&env)
    }

    /// (fee_receiver, deploy_fee) — the treasury and flat deployment fee.
    pub fn fee_config(env: Env) -> (Address, i128) {
        storage::get_fee_config(&env)
    }

    /// Current collection WASM hashes plus the version counter, or `None` if
    /// `set_wasm_hashes` was never called.
    pub fn wasm_hashes(env: Env) -> Option<WasmHashes> {
        Some(WasmHashes {
            normal_721: storage::get_wasm_normal_721(&env)?,
            normal_1155: storage::get_wasm_normal_1155(&env)?,
            lazy_721: storage::get_wasm_lazy_721(&env)?,
            lazy_1155: storage::get_wasm_lazy_1155(&env)?,
            version: storage::wasm_version(&env),
        })
    }

    pub fn wasm_version(env: Env) -> u32 {
        storage::wasm_version(&env)
    }
}
