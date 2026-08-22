use soroban_sdk::{symbol_short, Address, BytesN, Env};
use crate::types::CollectionKind;

// ── Event Schema Versioning (Issue #278) ────────────────────────────────────
//
// This mirrors the policy documented at the top of
// `contracts/soroban-marketplace/src/events.rs`: event shapes only ever
// change additively, numeric encodings are fixed once chosen, and topics are
// never reused for a different payload shape.
//
// Only `publish_deploy` is versioned today because it is the only launchpad
// event the indexer currently decodes (`indexer/src/parser.ts` `TOPIC_MAP`
// maps `dep_n721` / `dep_n1155` / `dep_l721` / `dep_l1155` to
// `DEPLOY_NORMAL_721` etc.). The other events below (fee collection, admin
// rotation, pause, wasm-hash updates) are emitted for on-chain audit history
// but are not yet read by the indexer; if/when indexer support is added for
// them, follow this same convention — append `EVENT_SCHEMA_VERSION` as a new
// trailing tuple element, never insert or reorder existing elements.
pub const EVENT_SCHEMA_VERSION: u32 = 1;

/// Emitted when a new collection is deployed via the launchpad.
///
/// Topics: ("deploy", kind_tag)
/// Data:   (creator: Address, deployed_address: Address, schema_version: u32)
///
/// `schema_version` was appended additively (Issue #278) as the 3rd tuple
/// element; historical events emitted before this change only have 2
/// elements, which the indexer's `DEPLOY_SCHEMA` still accepts (it validates
/// `length >= 2` and only type-checks indices 0 and 1).
#[allow(deprecated)]
pub fn publish_deploy(env: &Env, tag: soroban_sdk::Symbol, creator: &Address, address: &Address) {
    env.events().publish(
        (symbol_short!("deploy"), tag),
        (creator.clone(), address.clone(), EVENT_SCHEMA_VERSION),
    );
}

/// Emitted when a deployment fee is successfully transferred to the treasury.
///
/// Topics: ("fee_coll", creator, treasury)
/// Data:   (amount: i128, currency: Address)
#[allow(deprecated)]
pub fn publish_deployment_fee_collected(
    env: &Env,
    creator: &Address,
    treasury: &Address,
    amount: i128,
    currency: &Address,
) {
    env.events().publish(
        (symbol_short!("fee_coll"), creator.clone(), treasury.clone()),
        (amount, currency.clone()),
    );
}

/// Emitted when the admin updates the fee config.
///
/// Topics: ("cfg_fee",)
/// Data:   (receiver: Address, deploy_fee: i128)
#[allow(deprecated)]
pub fn publish_fee_config_updated(env: &Env, receiver: &Address, deploy_fee: i128) {
    env.events()
        .publish((symbol_short!("cfg_fee"),), (receiver.clone(), deploy_fee));
}

/// Emitted when the current admin proposes a successor.
///
/// Topics: ("admin", "proposed")
/// Data:   (current_admin: Address, proposed_admin: Address)
#[allow(deprecated)]
pub fn publish_admin_transfer_proposed(
    env: &Env,
    current_admin: &Address,
    proposed_admin: &Address,
) {
    env.events().publish(
        (symbol_short!("admin"), symbol_short!("proposed")),
        (current_admin.clone(), proposed_admin.clone()),
    );
}

/// Emitted when the proposed admin accepts the role.
///
/// Topics: ("admin", "accepted")
/// Data:   (old_admin: Address, new_admin: Address)
#[allow(deprecated)]
pub fn publish_admin_transfer_accepted(env: &Env, old_admin: &Address, new_admin: &Address) {
    env.events().publish(
        (symbol_short!("admin"), symbol_short!("accepted")),
        (old_admin.clone(), new_admin.clone()),
    );
}

/// Emitted when the current admin cancels a pending transfer.
///
/// Topics: ("admin", "cancelled")
/// Data:   (admin: Address, cancelled_pending: Address)
#[allow(deprecated)]
pub fn publish_admin_transfer_cancelled(env: &Env, admin: &Address, cancelled_pending: &Address) {
    env.events().publish(
        (symbol_short!("admin"), symbol_short!("cancelled")),
        (admin.clone(), cancelled_pending.clone()),
    );
}

/// Emitted when the admin pauses or unpauses deployments.
///
/// Topics: ("paused",)
/// Data:   (admin: Address, paused: bool)
#[allow(deprecated)]
pub fn publish_paused(env: &Env, admin: &Address, paused: bool) {
    env.events()
        .publish((symbol_short!("paused"),), (admin.clone(), paused));
}

/// Emitted when the admin assigns the `EmergencyPause` role to a new address
/// (Issue #267).
///
/// Topics: ("pauser", "set")
/// Data:   (pauser: Address)
#[allow(deprecated)]
pub fn publish_emergency_pauser_updated(env: &Env, pauser: &Address) {
    env.events()
        .publish((symbol_short!("pauser"), symbol_short!("set")), pauser.clone());
}

/// Emitted when the admin records a new set of collection WASM hashes.
///
/// Topics: ("wasm_set", version: u32)
/// Data:   (normal_721, normal_1155, lazy_721, lazy_1155)
#[allow(deprecated)]
pub fn publish_wasm_hashes_set(
    env: &Env,
    version: u32,
    normal_721: &BytesN<32>,
    normal_1155: &BytesN<32>,
    lazy_721: &BytesN<32>,
    lazy_1155: &BytesN<32>,
) {
    env.events().publish(
        (symbol_short!("wasm_set"), version),
        (
            normal_721.clone(),
            normal_1155.clone(),
            lazy_721.clone(),
            lazy_1155.clone(),
        ),
    );
}

/// Emitted when the admin updates the WASM hash for a specific collection kind.
///
/// Topics: ("wasm_upd", kind_tag)
/// Data:   (old_wasm: BytesN<32>, new_wasm: BytesN<32>)
#[allow(deprecated)]
pub fn publish_collection_wasm_updated(
    env: &Env,
    kind: &CollectionKind,
    old_wasm: &BytesN<32>,
    new_wasm: &BytesN<32>,
) {
    let tag = match kind {
        CollectionKind::Normal721 => symbol_short!("n721"),
        CollectionKind::Normal1155 => symbol_short!("n1155"),
        CollectionKind::LazyMint721 => symbol_short!("l721"),
        CollectionKind::LazyMint1155 => symbol_short!("l1155"),
    };
    env.events().publish(
        (symbol_short!("wasm_upd"), tag),
        (old_wasm.clone(), new_wasm.clone()),
    );
}

/// Emitted when the admin triggers an upgrade on a specific deployed collection.
///
/// Topics: ("col_upgr", collection_address)
/// Data:   (from_wasm: BytesN<32>, to_wasm: BytesN<32>)
#[allow(deprecated)]
pub fn publish_collection_upgraded(
    env: &Env,
    collection_address: &Address,
    from_wasm: &BytesN<32>,
    to_wasm: &BytesN<32>,
) {
    env.events().publish(
        (symbol_short!("col_upgr"), collection_address.clone()),
        (from_wasm.clone(), to_wasm.clone()),
    );
}

/// Emitted when a versioned migration completes successfully.
///
/// Topics: ("migrated", version: String)
/// Data:   ()
#[allow(deprecated)]
pub fn publish_migration_completed(env: &Env, version: &soroban_sdk::String) {
    env.events().publish(
        (soroban_sdk::symbol_short!("migrated"), version.clone()),
        (),
    );
}
