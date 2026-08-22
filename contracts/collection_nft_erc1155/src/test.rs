extern crate std;

use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger as _},
    Address, Env, String, Vec,
};

use crate::{DataKey, Error, NormalNFT1155, NormalNFT1155Client};

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn jump_ledger(env: &Env, delta: u32) {
    env.ledger().with_mut(|li| {
        li.sequence_number += delta;
    });
}

fn setup() -> (Env, NormalNFT1155Client<'static>, Address, Address) {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    env.mock_all_auths();
    let contract_id = env.register(NormalNFT1155, ());
    let client = NormalNFT1155Client::new(&env, &contract_id);
    let creator = Address::generate(&env);
    let royalty_receiver = Address::generate(&env);
    client.initialize(
        &creator,
        &String::from_str(&env, "Test 1155"),
        &500u32,
        &royalty_receiver,
    );
    (env, client, contract_id, creator)
}

// ─── Initializer ─────────────────────────────────────────────────────────────

#[test]
fn cannot_initialize_twice() {
    let (env, client, _, creator) = setup();
    let recv = Address::generate(&env);
    let result = client.try_initialize(&creator, &String::from_str(&env, "Again"), &0u32, &recv);
    assert_eq!(result, Err(Ok(Error::AlreadyInitialized)));
}

#[test]
fn name_is_stored_correctly() {
    let (env, client, _, _) = setup();
    assert_eq!(client.name(), String::from_str(&env, "Test 1155"));
}

#[test]
fn creator_is_stored_correctly() {
    let (_, client, _, creator) = setup();
    assert_eq!(client.creator(), creator);
}

// ─── Pre-existing batch semantics regression ──────────────────────────────────

#[test]
fn test_mint_batch_success_multiple() {
    let (env, client, _, _) = setup();
    let alice = Address::generate(&env);
    let token_ids = Vec::from_array(&env, [0u64, 1u64]);
    let amounts = Vec::from_array(&env, [100u128, 200u128]);
    let uris = Vec::from_array(&env, [
        String::from_str(&env, "uri-0"),
        String::from_str(&env, "uri-1"),
    ]);
    client.mint_batch(&alice, &token_ids, &amounts, &uris);
    assert_eq!(client.balance_of(&alice, &0u64), 100);
    assert_eq!(client.balance_of(&alice, &1u64), 200);
}

#[test]
fn test_mint_batch_try_success() {
    let (env, client, _, _) = setup();
    let alice = Address::generate(&env);
    let ids = Vec::from_array(&env, [0u64]);
    let amts = Vec::from_array(&env, [50u128]);
    let uris = Vec::from_array(&env, [String::from_str(&env, "uri-0")]);
    assert!(client.try_mint_batch(&alice, &ids, &amts, &uris).is_ok());
}

#[test]
fn test_mint_batch_length_mismatch_fails() {
    let (env, client, _, _) = setup();
    let alice = Address::generate(&env);
    let ids = Vec::from_array(&env, [0u64, 1u64]);
    let amts = Vec::from_array(&env, [100u128]);
    let uris = Vec::from_array(&env, [String::from_str(&env, "uri")]);
    assert!(client.try_mint_batch(&alice, &ids, &amts, &uris).is_err());
}

#[test]
fn test_mint_batch_empty_returns_empty_batch_error() {
    let (env, client, _, _) = setup();
    let alice = Address::generate(&env);
    let empty_ids: Vec<u64> = Vec::new(&env);
    let empty_amounts: Vec<u128> = Vec::new(&env);
    let empty_uris: Vec<String> = Vec::new(&env);
    let result = client.try_mint_batch(&alice, &empty_ids, &empty_amounts, &empty_uris);
    assert_eq!(result, Err(Ok(Error::EmptyBatch)));
}

#[test]
fn test_existing_token_does_not_override_uri() {
    let (env, client, _, _) = setup();
    let alice = Address::generate(&env);
    let ids = Vec::from_array(&env, [0u64]);
    let amounts = Vec::from_array(&env, [100u128]);
    let uris = Vec::from_array(&env, [String::from_str(&env, "original")]);
    client.mint_batch(&alice, &ids, &amounts, &uris);
    let new_uris = Vec::from_array(&env, [String::from_str(&env, "new")]);
    let amounts2 = Vec::from_array(&env, [50u128]);
    client.mint_batch(&alice, &ids, &amounts2, &new_uris);
    assert_eq!(client.uri(&0u64), String::from_str(&env, "original"));
    assert_eq!(client.total_supply(&0u64), 150);
}

// ─── TTL persistence ──────────────────────────────────────────────────────────

#[test]
fn test_ttl_persistence() {
    let (env, client, contract_id, _) = setup();
    let alice = Address::generate(&env);
    let token_id = client.mint_new(&alice, &10u128, &String::from_str(&env, "uri"));
    jump_ledger(&env, 60_000);
    let exists = env.as_contract(&contract_id, || {
        env.storage().persistent().has(&DataKey::TotalSupply(token_id))
    });
    assert!(exists);
}

#[test]
fn instance_ttl_is_extended_on_mint_new() {
    let (env, client, _, _) = setup();
    let alice = Address::generate(&env);
    jump_ledger(&env, 60_000);
    let id0 = client.mint_new(&alice, &10u128, &String::from_str(&env, "uri-0"));
    jump_ledger(&env, 60_000);
    let id1 = client.mint_new(&alice, &5u128, &String::from_str(&env, "uri-1"));
    assert_eq!(id0, 0u64);
    assert_eq!(id1, 1u64);
}

#[test]
fn persistent_ttl_is_extended_on_transfer_and_mint_keys() {
    let (env, client, contract_id, _) = setup();
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let tid = client.mint_new(&alice, &10u128, &String::from_str(&env, "uri"));
    client.transfer(&alice, &bob, &tid, &3u128);
    jump_ledger(&env, 60_000);
    let (a_exists, s_exists) = env.as_contract(&contract_id, || {
        (
            env.storage().persistent().has(&DataKey::Balance(alice.clone(), tid)),
            env.storage().persistent().has(&DataKey::TotalSupply(tid)),
        )
    });
    assert!(a_exists);
    assert!(s_exists);
}

#[test]
fn persistent_ttl_is_extended_on_burn_keys() {
    let (env, client, contract_id, _) = setup();
    let alice = Address::generate(&env);
    let tid = client.mint_new(&alice, &10u128, &String::from_str(&env, "uri"));
    client.burn(&alice, &alice, &tid, &4u128);
    jump_ledger(&env, 60_000);
    let (a_exists, s_exists) = env.as_contract(&contract_id, || {
        (
            env.storage().persistent().has(&DataKey::Balance(alice.clone(), tid)),
            env.storage().persistent().has(&DataKey::TotalSupply(tid)),
        )
    });
    assert!(a_exists);
    assert!(s_exists);
}

// ─── Events ──────────────────────────────────────────────────────────────────

#[test]
fn test_erc1155_events_emit_successfully() {
    let (env, client, _, _) = setup();
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let tid = client.mint_new(&alice, &100u128, &String::from_str(&env, "uri"));
    client.transfer(&alice, &bob, &tid, &30u128);
    client.burn(&bob, &bob, &tid, &10u128);
    assert_eq!(client.balance_of(&alice, &tid), 70u128);
    assert_eq!(client.balance_of(&bob, &tid), 20u128);
}

// ─── Burn regression #273 ─────────────────────────────────────────────────────

#[test]
fn burn_with_missing_total_supply_key_returns_zero_not_amount() {
    let (env, client, contract_id, _) = setup();
    let alice = Address::generate(&env);
    let tid = client.mint_new(&alice, &5u128, &String::from_str(&env, "uri"));
    env.as_contract(&contract_id, || {
        env.storage().persistent().remove(&DataKey::TotalSupply(tid));
    });
    client.burn(&alice, &alice, &tid, &3u128);
    assert_eq!(client.total_supply(&tid), 0u128);
}
// ─── Supply cap and per-wallet limit ─────────────────────────────────────────

#[test]
fn max_supply_cap_enforced_on_mint() {
    let (env, client, _, _) = setup();
    let alice = Address::generate(&env);
    let tid = client.mint_new(&alice, &5u128, &String::from_str(&env, "uri-0"));
    client.set_token_max_supply(&tid, &10u128);
    client.mint(&alice, &tid, &5u128, &String::from_str(&env, "uri-0"));
    assert_eq!(client.total_supply(&tid), 10u128);
    assert_eq!(
        client.try_mint(&alice, &tid, &1u128, &String::from_str(&env, "uri-0")),
        Err(Ok(Error::MaxSupplyReached))
    );
}

#[test]
fn per_wallet_limit_enforced_on_mint() {
    let (env, client, _, _) = setup();
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    client.set_per_wallet_limit(&3u128);
    assert_eq!(client.per_wallet_limit(), 3u128);
    let tid = client.mint_new(&alice, &3u128, &String::from_str(&env, "uri-0"));
    assert_eq!(client.wallet_minted(&alice, &tid), 3u128);
    assert_eq!(
        client.try_mint(&alice, &tid, &1u128, &String::from_str(&env, "uri-0")),
        Err(Ok(Error::WalletLimitReached))
    );
    client.mint(&bob, &tid, &3u128, &String::from_str(&env, "uri-0"));
    assert_eq!(client.wallet_minted(&bob, &tid), 3u128);
}

#[test]
fn mint_batch_supply_cap_enforced() {
    let (env, client, _, _) = setup();
    let alice = Address::generate(&env);
    let tid = client.mint_new(&alice, &5u128, &String::from_str(&env, "uri-0"));
    client.set_token_max_supply(&tid, &10u128);
    let ids = Vec::from_array(&env, [tid]);
    let amts = Vec::from_array(&env, [6u128]);
    let uris = Vec::from_array(&env, [String::from_str(&env, "uri-0")]);
    assert_eq!(client.try_mint_batch(&alice, &ids, &amts, &uris), Err(Ok(Error::MaxSupplyReached)));
}

#[test]
fn wallet_minted_counter_accurate_across_multiple_mints() {
    let (env, client, _, _) = setup();
    let alice = Address::generate(&env);
    client.set_per_wallet_limit(&100u128);
    let tid = client.mint_new(&alice, &10u128, &String::from_str(&env, "uri-0"));
    assert_eq!(client.wallet_minted(&alice, &tid), 10u128);
    client.mint(&alice, &tid, &20u128, &String::from_str(&env, "uri-0"));
    assert_eq!(client.wallet_minted(&alice, &tid), 30u128);
    client.mint(&alice, &tid, &15u128, &String::from_str(&env, "uri-0"));
    assert_eq!(client.wallet_minted(&alice, &tid), 45u128);
    assert_eq!(client.total_supply(&tid), 45u128);
}

#[test]
fn total_supply_accurate_across_multiple_mints() {
    let (env, client, _, _) = setup();
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let tid = client.mint_new(&alice, &10u128, &String::from_str(&env, "uri-0"));
    client.mint(&bob, &tid, &20u128, &String::from_str(&env, "uri-0"));
    client.mint(&alice, &tid, &5u128, &String::from_str(&env, "uri-0"));
    assert_eq!(client.total_supply(&tid), 35u128);
    assert_eq!(client.balance_of(&alice, &tid), 15u128);
    assert_eq!(client.balance_of(&bob, &tid), 20u128);
}

#[test]
fn no_wallet_limit_allows_unlimited_mints() {
    let (env, client, _, _) = setup();
    let alice = Address::generate(&env);
    assert_eq!(client.per_wallet_limit(), 0u128);
    let tid = client.mint_new(&alice, &1000u128, &String::from_str(&env, "uri-0"));
    client.mint(&alice, &tid, &1000u128, &String::from_str(&env, "uri-0"));
    assert_eq!(client.total_supply(&tid), 2000u128);
}

#[test]
fn mint_zero_amount_returns_zero_amount_error() {
    let (env, client, _, _) = setup();
    let alice = Address::generate(&env);
    let tid = client.mint_new(&alice, &1u128, &String::from_str(&env, "uri"));
    assert_eq!(
        client.try_mint(&alice, &tid, &0u128, &String::from_str(&env, "uri")),
        Err(Ok(Error::ZeroAmount))
    );
}

#[test]
fn mint_new_zero_amount_registers_token_type_without_supply() {
    let (env, client, _, _) = setup();
    let alice = Address::generate(&env);
    let tid = client.mint_new(&alice, &0u128, &String::from_str(&env, "ipfs://type0"));
    assert_eq!(client.total_supply(&tid), 0u128);
    assert_eq!(client.balance_of(&alice, &tid), 0u128);
    let next = client.mint_new(&alice, &1u128, &String::from_str(&env, "ipfs://type1"));
    assert_eq!(next, 1u64);
}

// ─── Pause — auth matrix ──────────────────────────────────────────────────────

#[test]
fn is_paused_defaults_false() { let (_, c, _, _) = setup(); assert!(!c.is_paused()); }

#[test]
fn pause_sets_flag() { let (_, c, _, _) = setup(); c.pause(); assert!(c.is_paused()); }

#[test]
fn unpause_clears_flag() { let (_, c, _, _) = setup(); c.pause(); c.unpause(); assert!(!c.is_paused()); }

#[test]
fn pause_idempotent() { let (_, c, _, _) = setup(); c.pause(); c.pause(); assert!(c.is_paused()); }

#[test]
fn unpause_idempotent_never_paused() { let (_, c, _, _) = setup(); c.unpause(); assert!(!c.is_paused()); }

#[test]
fn pause_non_creator_fails() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let cid = env.register(NormalNFT1155, ());
    let c = NormalNFT1155Client::new(&env, &cid);
    let creator = Address::generate(&env);
    let recv = Address::generate(&env);
    c.initialize(&creator, &String::from_str(&env, "T"), &500u32, &recv);
    assert!(c.try_pause().is_err());
}

#[test]
fn unpause_non_creator_fails() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let cid = env.register(NormalNFT1155, ());
    let c = NormalNFT1155Client::new(&env, &cid);
    let creator = Address::generate(&env);
    let recv = Address::generate(&env);
    c.initialize(&creator, &String::from_str(&env, "T"), &500u32, &recv);
    assert!(c.try_unpause().is_err());
}

// ─── Pause gates every state-mutating function ───────────────────────────────

#[test]
fn mint_new_blocked_when_paused() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    c.pause();
    assert_eq!(c.try_mint_new(&alice, &1u128, &String::from_str(&env, "u")), Err(Ok(Error::CollectionPaused)));
}

#[test]
fn mint_blocked_when_paused() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let tid = c.mint_new(&alice, &1u128, &String::from_str(&env, "u"));
    c.pause();
    assert_eq!(c.try_mint(&alice, &tid, &1u128, &String::from_str(&env, "u")), Err(Ok(Error::CollectionPaused)));
}

#[test]
fn mint_batch_blocked_when_paused() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    c.pause();
    let ids = Vec::from_array(&env, [0u64]);
    let amts = Vec::from_array(&env, [1u128]);
    let uris = Vec::from_array(&env, [String::from_str(&env, "u")]);
    assert_eq!(c.try_mint_batch(&alice, &ids, &amts, &uris), Err(Ok(Error::CollectionPaused)));
}

#[test]
fn transfer_blocked_when_paused() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let tid = c.mint_new(&alice, &10u128, &String::from_str(&env, "u"));
    c.pause();
    assert_eq!(c.try_transfer(&alice, &bob, &tid, &1u128), Err(Ok(Error::CollectionPaused)));
}

#[test]
fn transfer_from_blocked_when_paused() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let op = Address::generate(&env);
    let tid = c.mint_new(&alice, &10u128, &String::from_str(&env, "u"));
    c.set_approval_for_all(&alice, &op, &true, &None::<u32>);
    c.pause();
    assert_eq!(c.try_transfer_from(&op, &alice, &bob, &tid, &1u128), Err(Ok(Error::CollectionPaused)));
}

#[test]
fn batch_transfer_blocked_when_paused() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let tid = c.mint_new(&alice, &10u128, &String::from_str(&env, "u"));
    c.pause();
    let ids = Vec::from_array(&env, [tid]);
    let amts = Vec::from_array(&env, [1u128]);
    assert_eq!(c.try_batch_transfer(&alice, &alice, &bob, &ids, &amts), Err(Ok(Error::CollectionPaused)));
}

#[test]
fn burn_blocked_when_paused() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let tid = c.mint_new(&alice, &10u128, &String::from_str(&env, "u"));
    c.pause();
    assert_eq!(c.try_burn(&alice, &alice, &tid, &1u128), Err(Ok(Error::CollectionPaused)));
}

#[test]
fn mint_new_succeeds_after_unpause() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    c.pause(); c.unpause();
    let id = c.mint_new(&alice, &5u128, &String::from_str(&env, "u"));
    assert_eq!(c.balance_of(&alice, &id), 5u128);
}

#[test]
fn multiple_pause_unpause_cycles_work() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    c.pause();
    assert!(c.try_mint_new(&alice, &1u128, &String::from_str(&env, "u")).is_err());
    c.unpause();
    c.mint_new(&alice, &1u128, &String::from_str(&env, "u"));
    c.pause();
    assert!(c.try_mint_new(&alice, &1u128, &String::from_str(&env, "u")).is_err());
    c.unpause();
    c.mint_new(&alice, &1u128, &String::from_str(&env, "u"));
}

#[test]
fn pause_interleaved_with_batch_ops() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let ids = Vec::from_array(&env, [0u64, 1u64]);
    let amts = Vec::from_array(&env, [10u128, 20u128]);
    let uris = Vec::from_array(&env, [String::from_str(&env, "u0"), String::from_str(&env, "u1")]);
    c.mint_batch(&alice, &ids, &amts, &uris);
    c.pause();
    assert_eq!(c.try_batch_transfer(&alice, &alice, &bob, &ids, &amts), Err(Ok(Error::CollectionPaused)));
    c.unpause();
    let sub = Vec::from_array(&env, [5u128, 5u128]);
    c.batch_transfer(&alice, &alice, &bob, &ids, &sub);
    assert_eq!(c.balance_of(&bob, &0u64), 5u128);
    assert_eq!(c.balance_of(&bob, &1u64), 5u128);
}

#[test]
fn views_callable_while_paused() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let id = c.mint_new(&alice, &10u128, &String::from_str(&env, "u"));
    c.pause();
    assert_eq!(c.balance_of(&alice, &id), 10u128);
    assert_eq!(c.total_supply(&id), 10u128);
    assert_eq!(c.next_token_id(), 1u64);
    assert!(c.is_paused());
    assert!(!c.is_metadata_frozen());
    let _ = c.royalty_info();
    let _ = c.royalty_info_for(&id, &1000i128);
}

// ─── Metadata management ──────────────────────────────────────────────────────

#[test]
fn base_uri_initially_none() { let (_, c, _, _) = setup(); assert_eq!(c.base_uri(), None); }

#[test]
fn set_base_uri_stores_value() {
    let (env, c, _, _) = setup();
    c.set_base_uri(&String::from_str(&env, "ipfs://base/"));
    assert_eq!(c.base_uri(), Some(String::from_str(&env, "ipfs://base/")));
}

#[test]
fn set_base_uri_can_be_updated_before_freeze() {
    let (env, c, _, _) = setup();
    c.set_base_uri(&String::from_str(&env, "ipfs://v1/"));
    c.set_base_uri(&String::from_str(&env, "ipfs://v2/"));
    assert_eq!(c.base_uri(), Some(String::from_str(&env, "ipfs://v2/")));
}

#[test]
fn set_base_uri_non_creator_fails() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let cid = env.register(NormalNFT1155, ());
    let c = NormalNFT1155Client::new(&env, &cid);
    let creator = Address::generate(&env);
    let recv = Address::generate(&env);
    c.initialize(&creator, &String::from_str(&env, "T"), &0u32, &recv);
    assert!(c.try_set_base_uri(&String::from_str(&env, "ipfs://bad/")).is_err());
}

#[test]
fn is_metadata_frozen_defaults_false() { let (_, c, _, _) = setup(); assert!(!c.is_metadata_frozen()); }

#[test]
fn freeze_metadata_sets_frozen_flag() { let (_, c, _, _) = setup(); c.freeze_metadata(); assert!(c.is_metadata_frozen()); }

#[test]
fn freeze_metadata_twice_returns_already_frozen() {
    let (_, c, _, _) = setup();
    c.freeze_metadata();
    assert_eq!(c.try_freeze_metadata(), Err(Ok(Error::AlreadyFrozen)));
}

#[test]
fn set_base_uri_after_freeze_returns_metadata_frozen() {
    let (env, c, _, _) = setup();
    c.freeze_metadata();
    assert_eq!(c.try_set_base_uri(&String::from_str(&env, "ipfs://late/")), Err(Ok(Error::MetadataFrozen)));
}

#[test]
fn set_base_uri_before_freeze_then_frozen_rejects_update() {
    let (env, c, _, _) = setup();
    c.set_base_uri(&String::from_str(&env, "ipfs://ok/"));
    c.freeze_metadata();
    assert_eq!(c.try_set_base_uri(&String::from_str(&env, "ipfs://nope/")), Err(Ok(Error::MetadataFrozen)));
    assert_eq!(c.base_uri(), Some(String::from_str(&env, "ipfs://ok/")));
}

#[test]
fn freeze_metadata_non_creator_fails() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let cid = env.register(NormalNFT1155, ());
    let c = NormalNFT1155Client::new(&env, &cid);
    let creator = Address::generate(&env);
    let recv = Address::generate(&env);
    c.initialize(&creator, &String::from_str(&env, "T"), &0u32, &recv);
    assert!(c.try_freeze_metadata().is_err());
}

#[test]
fn freeze_persists_after_ownership_transfer() {
    let (env, c, _, _) = setup();
    let new_creator = Address::generate(&env);
    c.set_base_uri(&String::from_str(&env, "ipfs://frozen/"));
    c.freeze_metadata();
    c.transfer_ownership(&new_creator);
    assert!(c.is_metadata_frozen());
    assert_eq!(c.try_set_base_uri(&String::from_str(&env, "ipfs://nope/")), Err(Ok(Error::MetadataFrozen)));
}

#[test]
fn freeze_then_set_base_uri_reverts() {
    let (env, c, _, _) = setup();
    c.freeze_metadata();
    assert_eq!(c.try_set_base_uri(&String::from_str(&env, "ipfs://any/")), Err(Ok(Error::MetadataFrozen)));
}

// ─── set_token_uri ────────────────────────────────────────────────────────────

#[test]
fn set_token_uri_updates_stored_uri() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let id = c.mint_new(&alice, &1u128, &String::from_str(&env, "old"));
    c.set_token_uri(&id, &String::from_str(&env, "new"));
    assert_eq!(c.uri(&id), String::from_str(&env, "new"));
}

#[test]
fn set_token_uri_blocked_after_collection_freeze() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let id = c.mint_new(&alice, &1u128, &String::from_str(&env, "uri"));
    c.freeze_metadata();
    assert_eq!(c.try_set_token_uri(&id, &String::from_str(&env, "new")), Err(Ok(Error::MetadataFrozen)));
}

#[test]
fn set_token_uri_blocked_after_token_freeze() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let id = c.mint_new(&alice, &1u128, &String::from_str(&env, "uri"));
    c.freeze_token(&alice, &id);
    assert_eq!(c.try_set_token_uri(&id, &String::from_str(&env, "new")), Err(Ok(Error::MetadataFrozen)));
}

#[test]
fn set_token_uri_on_nonexistent_token_fails() {
    let (env, c, _, _) = setup();
    assert_eq!(c.try_set_token_uri(&999u64, &String::from_str(&env, "uri")), Err(Ok(Error::TokenNotFound)));
}

#[test]
fn set_token_uri_non_creator_fails() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let cid = env.register(NormalNFT1155, ());
    let c = NormalNFT1155Client::new(&env, &cid);
    let creator = Address::generate(&env);
    let recv = Address::generate(&env);
    c.initialize(&creator, &String::from_str(&env, "T"), &0u32, &recv);
    assert!(c.try_set_token_uri(&0u64, &String::from_str(&env, "uri")).is_err());
}

// ─── freeze_token ─────────────────────────────────────────────────────────────

#[test]
fn freeze_token_by_holder_succeeds() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let id = c.mint_new(&alice, &1u128, &String::from_str(&env, "uri"));
    assert!(!c.is_token_frozen(&id));
    c.freeze_token(&alice, &id);
    assert!(c.is_token_frozen(&id));
}

#[test]
fn freeze_token_twice_returns_already_frozen() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let id = c.mint_new(&alice, &1u128, &String::from_str(&env, "uri"));
    c.freeze_token(&alice, &id);
    assert_eq!(c.try_freeze_token(&alice, &id), Err(Ok(Error::AlreadyFrozen)));
}

#[test]
fn freeze_token_on_nonexistent_token_fails() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    assert_eq!(c.try_freeze_token(&alice, &999u64), Err(Ok(Error::TokenNotFound)));
}

#[test]
fn freeze_token_by_non_holder_non_creator_fails() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let eve = Address::generate(&env);
    let id = c.mint_new(&alice, &1u128, &String::from_str(&env, "uri"));
    assert_eq!(c.try_freeze_token(&eve, &id), Err(Ok(Error::NotApproved)));
}

#[test]
fn token_freeze_persists_after_ownership_transfer() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let new_owner = Address::generate(&env);
    let id = c.mint_new(&alice, &1u128, &String::from_str(&env, "uri"));
    c.freeze_token(&alice, &id);
    c.transfer_ownership(&new_owner);
    assert_eq!(c.try_set_token_uri(&id, &String::from_str(&env, "new")), Err(Ok(Error::MetadataFrozen)));
}

// ─── URI resolution order ─────────────────────────────────────────────────────

#[test]
fn uri_returns_per_token_uri_when_no_base_uri() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let id = c.mint_new(&alice, &1u128, &String::from_str(&env, "ipfs://Qm123"));
    assert_eq!(c.uri(&id), String::from_str(&env, "ipfs://Qm123"));
}

#[test]
fn uri_returns_base_uri_plus_token_id_when_base_set() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    c.set_base_uri(&String::from_str(&env, "https://api.example.com/metadata/"));
    let id = c.mint_new(&alice, &1u128, &String::from_str(&env, "ignored"));
    assert_eq!(c.uri(&id), String::from_str(&env, "https://api.example.com/metadata/0"));
}

#[test]
fn uri_base_uri_with_multiple_tokens() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    c.set_base_uri(&String::from_str(&env, "ipfs://col/"));
    c.mint_new(&alice, &1u128, &String::from_str(&env, "u0"));
    c.mint_new(&alice, &1u128, &String::from_str(&env, "u1"));
    c.mint_new(&alice, &1u128, &String::from_str(&env, "u2"));
    assert_eq!(c.uri(&0u64), String::from_str(&env, "ipfs://col/0"));
    assert_eq!(c.uri(&1u64), String::from_str(&env, "ipfs://col/1"));
    assert_eq!(c.uri(&2u64), String::from_str(&env, "ipfs://col/2"));
}

#[test]
fn uri_frozen_base_uri_still_resolves_correctly() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    c.set_base_uri(&String::from_str(&env, "ipfs://frozen/"));
    let id = c.mint_new(&alice, &1u128, &String::from_str(&env, "u"));
    c.freeze_metadata();
    assert_eq!(c.uri(&id), String::from_str(&env, "ipfs://frozen/0"));
    assert_eq!(c.base_uri(), Some(String::from_str(&env, "ipfs://frozen/")));
}

#[test]
fn uri_base_uri_update_changes_resolution_for_all_tokens() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    c.mint_new(&alice, &1u128, &String::from_str(&env, "old-0"));
    c.mint_new(&alice, &1u128, &String::from_str(&env, "old-1"));
    assert_eq!(c.uri(&0u64), String::from_str(&env, "old-0"));
    c.set_base_uri(&String::from_str(&env, "https://new/"));
    assert_eq!(c.uri(&0u64), String::from_str(&env, "https://new/0"));
    assert_eq!(c.uri(&1u64), String::from_str(&env, "https://new/1"));
}

#[test]
fn uri_boundary_token_id_zero() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    c.set_base_uri(&String::from_str(&env, "https://x/"));
    let id = c.mint_new(&alice, &1u128, &String::from_str(&env, "u"));
    assert_eq!(id, 0u64);
    assert_eq!(c.uri(&id), String::from_str(&env, "https://x/0"));
}

// ─── Per-token royalties (ERC-2981 parity) ────────────────────────────────────

#[test]
fn royalty_info_returns_initialized_defaults() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    env.mock_all_auths();
    let cid = env.register(NormalNFT1155, ());
    let c = NormalNFT1155Client::new(&env, &cid);
    let creator = Address::generate(&env);
    let recv = Address::generate(&env);
    c.initialize(&creator, &String::from_str(&env, "T"), &750u32, &recv);
    let (r, bps) = c.royalty_info();
    assert_eq!(r, recv);
    assert_eq!(bps, 750u32);
}

#[test]
fn royalty_info_for_uses_default_when_no_per_token_override() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let id = c.mint_new(&alice, &1u128, &String::from_str(&env, "uri"));
    let (default_recv, _) = c.royalty_info();
    let (recv, amount) = c.royalty_info_for(&id, &10_000i128);
    assert_eq!(recv, default_recv);
    assert_eq!(amount, 500i128);
}

#[test]
fn royalty_info_for_per_token_override_wins_over_default() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let token_recv = Address::generate(&env);
    let id = c.mint_new(&alice, &1u128, &String::from_str(&env, "uri"));
    c.set_token_royalty(&id, &token_recv, &1_000u32);
    let (recv, amount) = c.royalty_info_for(&id, &5_000i128);
    assert_eq!(recv, token_recv);
    assert_eq!(amount, 500i128);
}

#[test]
fn royalty_info_for_another_token_uses_default() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let token_recv = Address::generate(&env);
    let id0 = c.mint_new(&alice, &1u128, &String::from_str(&env, "u0"));
    let id1 = c.mint_new(&alice, &1u128, &String::from_str(&env, "u1"));
    c.set_token_royalty(&id0, &token_recv, &200u32);
    let (default_recv, _) = c.royalty_info();
    let (r1, _) = c.royalty_info_for(&id1, &1_000i128);
    assert_eq!(r1, default_recv);
}

#[test]
fn set_default_royalty_updates_collection_royalty() {
    let (env, c, _, _) = setup();
    let new_recv = Address::generate(&env);
    c.set_default_royalty(&new_recv, &300u32);
    let (recv, bps) = c.royalty_info();
    assert_eq!(recv, new_recv);
    assert_eq!(bps, 300u32);
}

#[test]
fn set_default_royalty_reflected_in_royalty_info_for() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let new_recv = Address::generate(&env);
    let id = c.mint_new(&alice, &1u128, &String::from_str(&env, "uri"));
    c.set_default_royalty(&new_recv, &250u32);
    let (recv, amount) = c.royalty_info_for(&id, &8_000i128);
    assert_eq!(recv, new_recv);
    assert_eq!(amount, 200i128);
}

#[test]
fn set_default_royalty_zero_bps_returns_zero_amount() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let recv = Address::generate(&env);
    let id = c.mint_new(&alice, &1u128, &String::from_str(&env, "uri"));
    c.set_default_royalty(&recv, &0u32);
    let (_, amount) = c.royalty_info_for(&id, &100_000i128);
    assert_eq!(amount, 0i128);
}

#[test]
fn set_default_royalty_max_bps_succeeds() {
    let (env, c, _, _) = setup();
    let recv = Address::generate(&env);
    assert!(c.try_set_default_royalty(&recv, &10_000u32).is_ok());
    let (_, bps) = c.royalty_info();
    assert_eq!(bps, 10_000u32);
}

#[test]
fn set_default_royalty_exceeds_max_bps_returns_invalid_bps() {
    let (env, c, _, _) = setup();
    let recv = Address::generate(&env);
    assert_eq!(c.try_set_default_royalty(&recv, &10_001u32), Err(Ok(Error::InvalidBps)));
}

#[test]
fn set_default_royalty_non_creator_fails() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let cid = env.register(NormalNFT1155, ());
    let c = NormalNFT1155Client::new(&env, &cid);
    let creator = Address::generate(&env);
    let recv = Address::generate(&env);
    c.initialize(&creator, &String::from_str(&env, "T"), &0u32, &recv);
    assert!(c.try_set_default_royalty(&recv, &500u32).is_err());
}

#[test]
fn set_token_royalty_zero_bps_returns_zero_amount() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let token_recv = Address::generate(&env);
    let id = c.mint_new(&alice, &1u128, &String::from_str(&env, "uri"));
    c.set_token_royalty(&id, &token_recv, &0u32);
    let (recv, amount) = c.royalty_info_for(&id, &999_999i128);
    assert_eq!(recv, token_recv);
    assert_eq!(amount, 0i128);
}

#[test]
fn set_token_royalty_max_bps_succeeds() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let token_recv = Address::generate(&env);
    let id = c.mint_new(&alice, &1u128, &String::from_str(&env, "uri"));
    assert!(c.try_set_token_royalty(&id, &token_recv, &10_000u32).is_ok());
    let (recv, amount) = c.royalty_info_for(&id, &1_000i128);
    assert_eq!(recv, token_recv);
    assert_eq!(amount, 1_000i128);
}

#[test]
fn set_token_royalty_exceeds_max_bps_returns_invalid_bps() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let token_recv = Address::generate(&env);
    let id = c.mint_new(&alice, &1u128, &String::from_str(&env, "uri"));
    assert_eq!(c.try_set_token_royalty(&id, &token_recv, &10_001u32), Err(Ok(Error::InvalidBps)));
}

#[test]
fn set_token_royalty_non_creator_fails() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let cid = env.register(NormalNFT1155, ());
    let c = NormalNFT1155Client::new(&env, &cid);
    let creator = Address::generate(&env);
    let recv = Address::generate(&env);
    c.initialize(&creator, &String::from_str(&env, "T"), &0u32, &recv);
    assert!(c.try_set_token_royalty(&0u64, &recv, &500u32).is_err());
}

#[test]
fn royalty_info_for_zero_sale_price_returns_zero_amount() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let id = c.mint_new(&alice, &1u128, &String::from_str(&env, "uri"));
    let (_, amount) = c.royalty_info_for(&id, &0i128);
    assert_eq!(amount, 0i128);
}

#[test]
fn royalty_info_for_rounds_down_fractional_amount() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let recv = Address::generate(&env);
    let id = c.mint_new(&alice, &1u128, &String::from_str(&env, "uri"));
    c.set_default_royalty(&recv, &333u32);
    let (_, amount) = c.royalty_info_for(&id, &1_000i128);
    assert_eq!(amount, 33i128);
}

#[test]
fn per_token_royalty_can_be_updated() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let recv1 = Address::generate(&env);
    let recv2 = Address::generate(&env);
    let id = c.mint_new(&alice, &1u128, &String::from_str(&env, "uri"));
    c.set_token_royalty(&id, &recv1, &100u32);
    let (r1, _) = c.royalty_info_for(&id, &1_000i128);
    assert_eq!(r1, recv1);
    c.set_token_royalty(&id, &recv2, &200u32);
    let (r2, amount) = c.royalty_info_for(&id, &1_000i128);
    assert_eq!(r2, recv2);
    assert_eq!(amount, 20i128);
}

#[test]
fn default_royalty_does_not_affect_per_token_override() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let token_recv = Address::generate(&env);
    let new_default = Address::generate(&env);
    let id = c.mint_new(&alice, &1u128, &String::from_str(&env, "uri"));
    c.set_token_royalty(&id, &token_recv, &800u32);
    c.set_default_royalty(&new_default, &100u32);
    let (recv, amount) = c.royalty_info_for(&id, &10_000i128);
    assert_eq!(recv, token_recv);
    assert_eq!(amount, 800i128);
}

#[test]
fn royalty_math_property_checks_match_721_rounding() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let recv = Address::generate(&env);
    let id = c.mint_new(&alice, &1u128, &String::from_str(&env, "uri"));
    let cases: &[(u32, i128, i128)] = &[
        (0, 1, 0), (0, 1_000_000, 0),
        (1, 1, 0), (1, 10_000, 1),
        (250, 1, 0), (250, 10_000, 250), (250, 100_000, 2_500),
        (10_000, 1, 1), (10_000, 10_000, 10_000),
        (500, i128::MAX / 10_000, (i128::MAX / 10_000) * 500 / 10_000),
    ];
    for &(bps, sale_price, expected) in cases {
        c.set_default_royalty(&recv, &bps);
        let (_, amount) = c.royalty_info_for(&id, &sale_price);
        assert_eq!(amount, expected, "bps={bps} price={sale_price}: expected {expected} got {amount}");
    }
}

// ─── update_royalty backward compatibility ────────────────────────────────────

#[test]
fn update_royalty_still_works_as_legacy_setter() {
    let (env, c, _, _) = setup();
    let new_recv = Address::generate(&env);
    c.update_royalty(&new_recv, &250u32);
    let (recv, bps) = c.royalty_info();
    assert_eq!(recv, new_recv);
    assert_eq!(bps, 250u32);
}

#[test]
fn update_royalty_is_reflected_in_royalty_info_for() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let new_recv = Address::generate(&env);
    let id = c.mint_new(&alice, &1u128, &String::from_str(&env, "uri"));
    c.update_royalty(&new_recv, &100u32);
    let (recv, amount) = c.royalty_info_for(&id, &10_000i128);
    assert_eq!(recv, new_recv);
    assert_eq!(amount, 100i128);
}

// ─── mint_batch invariant hardening — duplicate IDs ───────────────────────────

#[test]
fn mint_batch_duplicate_ids_cannot_exceed_max_supply() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let tid = c.mint_new(&alice, &1u128, &String::from_str(&env, "uri"));
    c.set_token_max_supply(&tid, &5u128);
    let ids = Vec::from_array(&env, [tid, tid]);
    let amts = Vec::from_array(&env, [3u128, 3u128]);
    let uris = Vec::from_array(&env, [String::from_str(&env, "u"), String::from_str(&env, "u")]);
    assert_eq!(c.try_mint_batch(&alice, &ids, &amts, &uris), Err(Ok(Error::MaxSupplyReached)));
    assert_eq!(c.total_supply(&tid), 1u128);
}

#[test]
fn mint_batch_duplicate_ids_exactly_at_max_supply_succeeds() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let tid = c.mint_new(&alice, &0u128, &String::from_str(&env, "uri"));
    c.set_token_max_supply(&tid, &6u128);
    let ids = Vec::from_array(&env, [tid, tid]);
    let amts = Vec::from_array(&env, [3u128, 3u128]);
    let uris = Vec::from_array(&env, [String::from_str(&env, "u"), String::from_str(&env, "u")]);
    c.mint_batch(&alice, &ids, &amts, &uris);
    assert_eq!(c.total_supply(&tid), 6u128);
    let ids2 = Vec::from_array(&env, [tid]);
    let amts2 = Vec::from_array(&env, [1u128]);
    let uris2 = Vec::from_array(&env, [String::from_str(&env, "u")]);
    assert_eq!(c.try_mint_batch(&alice, &ids2, &amts2, &uris2), Err(Ok(Error::MaxSupplyReached)));
}

#[test]
fn mint_batch_duplicate_ids_cannot_exceed_wallet_limit() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    c.set_per_wallet_limit(&5u128);
    let tid = c.mint_new(&alice, &1u128, &String::from_str(&env, "uri"));
    let ids = Vec::from_array(&env, [tid, tid]);
    let amts = Vec::from_array(&env, [3u128, 3u128]);
    let uris = Vec::from_array(&env, [String::from_str(&env, "u"), String::from_str(&env, "u")]);
    assert_eq!(c.try_mint_batch(&alice, &ids, &amts, &uris), Err(Ok(Error::WalletLimitReached)));
    assert_eq!(c.wallet_minted(&alice, &tid), 1u128);
}

#[test]
fn mint_batch_supply_boundary_mint_to_exactly_max_then_one_more_reverts() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let tid = c.mint_new(&alice, &0u128, &String::from_str(&env, "uri"));
    c.set_token_max_supply(&tid, &10u128);
    let ids = Vec::from_array(&env, [tid]);
    let amts = Vec::from_array(&env, [10u128]);
    let uris = Vec::from_array(&env, [String::from_str(&env, "u")]);
    c.mint_batch(&alice, &ids, &amts, &uris);
    assert_eq!(c.total_supply(&tid), 10u128);
    let ids2 = Vec::from_array(&env, [tid]);
    let amts2 = Vec::from_array(&env, [1u128]);
    let uris2 = Vec::from_array(&env, [String::from_str(&env, "u")]);
    assert_eq!(c.try_mint_batch(&alice, &ids2, &amts2, &uris2), Err(Ok(Error::MaxSupplyReached)));
}

// ─── balance_of_batch ────────────────────────────────────────────────────────

#[test]
fn balance_of_batch_returns_correct_values() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let t0 = c.mint_new(&alice, &10u128, &String::from_str(&env, "u0"));
    let t1 = c.mint_new(&bob, &20u128, &String::from_str(&env, "u1"));
    let accounts = Vec::from_array(&env, [alice.clone(), bob.clone()]);
    let ids = Vec::from_array(&env, [t0, t1]);
    let balances = c.balance_of_batch(&accounts, &ids);
    assert_eq!(balances.get(0).unwrap(), 10u128);
    assert_eq!(balances.get(1).unwrap(), 20u128);
}

#[test]
fn balance_of_batch_mismatched_lengths_returns_empty() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let accounts = Vec::from_array(&env, [alice]);
    let ids: Vec<u64> = Vec::from_array(&env, [0u64, 1u64]);
    assert_eq!(c.balance_of_batch(&accounts, &ids).len(), 0);
}

// ─── Approval management ──────────────────────────────────────────────────────

#[test]
fn set_approval_for_all_and_is_approved_for_all() {
    let (env, c, _, _) = setup();
    let owner = Address::generate(&env);
    let op = Address::generate(&env);
    assert!(!c.is_approved_for_all(&owner, &op));
    c.set_approval_for_all(&owner, &op, &true, &None::<u32>);
    assert!(c.is_approved_for_all(&owner, &op));
    c.set_approval_for_all(&owner, &op, &false, &None::<u32>);
    assert!(!c.is_approved_for_all(&owner, &op));
}

#[test]
fn transfer_from_by_operator_succeeds() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let op = Address::generate(&env);
    let id = c.mint_new(&alice, &10u128, &String::from_str(&env, "uri"));
    c.set_approval_for_all(&alice, &op, &true, &None::<u32>);
    c.transfer_from(&op, &alice, &bob, &id, &5u128);
    assert_eq!(c.balance_of(&bob, &id), 5u128);
    assert_eq!(c.balance_of(&alice, &id), 5u128);
}

#[test]
fn transfer_from_without_approval_fails() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let eve = Address::generate(&env);
    let id = c.mint_new(&alice, &10u128, &String::from_str(&env, "uri"));
    assert_eq!(c.try_transfer_from(&eve, &alice, &bob, &id, &1u128), Err(Ok(Error::NotApproved)));
}

#[test]
fn burn_by_operator_succeeds() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let op = Address::generate(&env);
    let id = c.mint_new(&alice, &10u128, &String::from_str(&env, "uri"));
    c.set_approval_for_all(&alice, &op, &true, &None::<u32>);
    c.burn(&op, &alice, &id, &3u128);
    assert_eq!(c.balance_of(&alice, &id), 7u128);
}

#[test]
fn burn_insufficient_balance_fails() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let id = c.mint_new(&alice, &5u128, &String::from_str(&env, "uri"));
    assert_eq!(c.try_burn(&alice, &alice, &id, &10u128), Err(Ok(Error::InsufficientBalance)));
}

// ─── Ownership management ─────────────────────────────────────────────────────

#[test]
fn transfer_ownership_updates_creator() {
    let (env, c, _, _) = setup();
    let new_creator = Address::generate(&env);
    c.transfer_ownership(&new_creator);
    assert_eq!(c.creator(), new_creator);
}

#[test]
fn transfer_ownership_non_creator_fails() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let cid = env.register(NormalNFT1155, ());
    let c = NormalNFT1155Client::new(&env, &cid);
    let creator = Address::generate(&env);
    let recv = Address::generate(&env);
    c.initialize(&creator, &String::from_str(&env, "T"), &0u32, &recv);
    assert!(c.try_transfer_ownership(&Address::generate(&env)).is_err());
}

#[test]
fn set_token_max_supply_non_creator_fails() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let cid = env.register(NormalNFT1155, ());
    let c = NormalNFT1155Client::new(&env, &cid);
    let creator = Address::generate(&env);
    let recv = Address::generate(&env);
    c.initialize(&creator, &String::from_str(&env, "T"), &0u32, &recv);
    assert!(c.try_set_token_max_supply(&0u64, &100u128).is_err());
}

#[test]
fn set_per_wallet_limit_non_creator_fails() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let cid = env.register(NormalNFT1155, ());
    let c = NormalNFT1155Client::new(&env, &cid);
    let creator = Address::generate(&env);
    let recv = Address::generate(&env);
    c.initialize(&creator, &String::from_str(&env, "T"), &0u32, &recv);
    assert!(c.try_set_per_wallet_limit(&10u128).is_err());
}

#[test]
fn set_token_max_supply_zero_removes_cap() {
    let (env, c, _, _) = setup();
    let alice = Address::generate(&env);
    let tid = c.mint_new(&alice, &5u128, &String::from_str(&env, "uri"));
    c.set_token_max_supply(&tid, &10u128);
    c.set_token_max_supply(&tid, &0u128);
    assert_eq!(c.token_max_supply(&tid), 0u128);
    c.mint(&alice, &tid, &100u128, &String::from_str(&env, "uri"));
    assert_eq!(c.total_supply(&tid), 105u128);
}

// ─── Migration ────────────────────────────────────────────────────────────────

mod migration {
    use super::*;

    #[test]
    fn fresh_install_migrate_records_version() {
        let (env, c, _, _) = setup();
        assert!(c.contract_version().is_none());
        c.migrate();
        assert_eq!(c.contract_version(), Some(String::from_str(&env, "1.0.0")));
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #13)")]
    fn double_migrate_reverts() {
        let (_, c, _, _) = setup();
        c.migrate();
        c.migrate();
    }

    #[test]
    fn migrate_emits_migrated_event() {
        let (env, c, _, _) = setup();
        c.migrate();
        use soroban_sdk::xdr::{ContractEventBody, ScVal};
        let events = env.events().all();
        let found = events.events().iter().any(|e| {
            if let ContractEventBody::V0(body) = &e.body {
                body.topics.iter().any(|t| match t {
                    ScVal::Symbol(s) => {
                        core::str::from_utf8(s.0.as_slice()).unwrap_or("") == "migrated"
                    }
                    _ => false,
                })
            } else {
                false
            }
        });
        assert!(found, "expected 'migrated' event");
    }

    #[test]
    fn balances_readable_after_migrate() {
        let (env, c, _, _) = setup();
        let alice = Address::generate(&env);
        c.mint_new(&alice, &10u128, &String::from_str(&env, "ipfs://token0"));
        c.migrate();
        assert_eq!(c.balance_of(&alice, &0u64), 10u128);
        assert_eq!(c.total_supply(&0u64), 10u128);
    }

    #[test]
    fn per_token_max_supply_readable_after_migrate() {
        let (env, c, _, _) = setup();
        c.set_token_max_supply(&0u64, &500u128);
        c.migrate();
        assert_eq!(c.token_max_supply(&0u64), 500u128);
    }

    #[test]
    fn royalty_readable_after_migrate() {
        let (_, c, _, _) = setup();
        c.migrate();
        let (_, bps) = c.royalty_info();
        assert_eq!(bps, 500u32);
    }

    #[test]
    fn pause_state_survives_migrate() {
        let (_, c, _, _) = setup();
        c.pause();
        c.migrate();
        assert!(c.is_paused());
    }

    #[test]
    fn metadata_frozen_state_survives_migrate() {
        let (_, c, _, _) = setup();
        c.freeze_metadata();
        c.migrate();
        assert!(c.is_metadata_frozen());
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Security hardening regressions (#6)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn initialize_royalty_bps_over_max_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(NormalNFT1155, ());
    let client = NormalNFT1155Client::new(&env, &contract_id);
    let creator = Address::generate(&env);
    let royalty_receiver = Address::generate(&env);
    let res = client.try_initialize(
        &creator,
        &String::from_str(&env, "Test"),
        &10_001u32, // > MAX_BPS
        &royalty_receiver,
    );
    assert_eq!(res, Err(Ok(Error::InvalidBps)));
}

#[test]
fn update_royalty_over_max_fails() {
    let (env, client, _, _) = setup();
    let new_receiver = Address::generate(&env);
    let res = client.try_update_royalty(&new_receiver, &10_001u32);
    assert_eq!(res, Err(Ok(Error::InvalidBps)));
}

#[test]
fn update_royalty_at_max_succeeds() {
    let (env, client, _, _) = setup();
    let new_receiver = Address::generate(&env);
    assert!(client.try_update_royalty(&new_receiver, &10_000u32).is_ok());
}

#[test]
fn metadata_frozen_blocks_set_base_uri() {
    let (env, client, _, _) = setup();
    client.freeze_metadata();
    let res = client.try_set_base_uri(&String::from_str(&env, "https://new.example/"));
    assert_eq!(res, Err(Ok(Error::MetadataFrozen)));
}

#[test]
fn paused_collection_blocks_mint_new() {
    let (env, client, _, _) = setup();
    client.pause();
    let alice = Address::generate(&env);
    let res = client.try_mint_new(&alice, &5u128, &String::from_str(&env, "uri"));
    assert_eq!(res, Err(Ok(Error::CollectionPaused)));
}

#[test]
fn batch_mint_all_or_nothing_rejects_invalid_uri() {
    let (env, client, _, _) = setup();
    let alice = Address::generate(&env);
    // Empty URI within a batch must cause EmptyUri, and no token should be minted
    let supply_before = client.next_token_id();
    let valid_uri = String::from_str(&env, "ipfs://valid");
    let empty_uri = String::from_str(&env, "");
    let mut token_ids = Vec::new(&env);
    let mut amounts = Vec::new(&env);
    let mut uris = Vec::new(&env);
    token_ids.push_back(0u64);
    token_ids.push_back(1u64);
    amounts.push_back(1u128);
    amounts.push_back(1u128);
    uris.push_back(valid_uri);
    uris.push_back(empty_uri);
    let res = client.try_mint_batch(&alice, &token_ids, &amounts, &uris);
    assert_eq!(res, Err(Ok(Error::EmptyUri)));
    // All-or-nothing: no tokens were minted (next_token_id unchanged)
    assert_eq!(client.next_token_id(), supply_before);
}
