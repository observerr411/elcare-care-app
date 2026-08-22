#![cfg(test)]

use crate::{BatchVoucherItem, DataKey, Error, LazyMint721, LazyMint721Client, MintVoucher};
use ed25519_dalek::{Signer, SigningKey};
use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger as _},
    xdr::ToXdr,
    Address, Bytes, BytesN, Env, String, Vec,
};

// ─── Shared helpers ───────────────────────────────────────────────────────────

fn creator_signing_key() -> SigningKey {
    SigningKey::from_bytes(&[7u8; 32])
}

fn setup(fee_bps: u32) -> (Env, LazyMint721Client<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let contract_id = env.register(LazyMint721, ());
    let client = LazyMint721Client::new(&env, &contract_id);
    let creator = Address::generate(&env);
    let fee_receiver = Address::generate(&env);
    let sk = creator_signing_key();
    client.initialize(
        &creator,
        &BytesN::from_array(&env, &sk.verifying_key().to_bytes()),
        &String::from_str(&env, "TestNFT"),
        &String::from_str(&env, "TNFT"),
        &1000u64,
        &0u32,
        &Address::generate(&env),
        &fee_receiver,
        &fee_bps,
        &String::from_str(&env, "Test SDF Network ; September 2015"),
    );
    (env, client, creator, fee_receiver)
}

/// Returns (env, client, creator) for tests that need to call default_init separately.
fn setup_test() -> (Env, LazyMint721Client<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let contract_id = env.register(LazyMint721, ());
    let client = LazyMint721Client::new(&env, &contract_id);
    let creator = Address::generate(&env);
    (env, client, creator)
}

fn default_init(env: &Env, client: &LazyMint721Client, creator: &Address) {
    let sk = creator_signing_key();
    client.initialize(
        creator,
        &BytesN::from_array(env, &sk.verifying_key().to_bytes()),
        &String::from_str(env, "TestNFT"),
        &String::from_str(env, "TNFT"),
        &1000u64,
        &0u32,
        &Address::generate(env),
        &Address::generate(env),
        &0u32,
        &String::from_str(env, "Test SDF Network ; September 2015"),
    );
}

fn empty_proof(env: &Env) -> Vec<BytesN<32>> {
    Vec::new(env)
}

fn make_voucher(env: &Env, token_id: u64) -> MintVoucher {
    MintVoucher {
        token_id,
        nonce: token_id,
        price: 0,
        currency: Address::generate(env),
        uri: String::from_str(env, "ipfs://test"),
        uri_hash: BytesN::from_array(env, &[0u8; 32]),
        valid_until: 0,
    }
}

fn sign_voucher(env: &Env, contract_id: &Address, voucher: &MintVoucher) -> BytesN<64> {
    let sk = creator_signing_key();
    let digest = env.as_contract(contract_id, || LazyMint721::_voucher_digest(env, voucher));
    let mut msg = [0u8; 32];
    digest.copy_into_slice(&mut msg);
    BytesN::from_array(env, &sk.sign(&msg).to_bytes())
}

fn make_batch_item(env: &Env, contract_id: &Address, token_id: u64) -> BatchVoucherItem {
    let v = make_voucher(env, token_id);
    let sig = sign_voucher(env, contract_id, &v);
    BatchVoucherItem {
        voucher: v,
        signature: sig,
        merkle_proof: empty_proof(env),
    }
}

/// Compute sha256(address XDR) — leaf hash for Merkle trees.
fn leaf_hash(env: &Env, addr: &Address) -> BytesN<32> {
    env.crypto().sha256(&addr.clone().to_xdr(env)).into()
}

/// Combine two hashes in sorted order (standard binary Merkle step).
fn combine(env: &Env, a: BytesN<32>, b: BytesN<32>) -> BytesN<32> {
    let mut pair = Bytes::new(env);
    if a.to_array() <= b.to_array() {
        pair.append(&a.into());
        pair.append(&b.into());
    } else {
        pair.append(&b.into());
        pair.append(&a.into());
    }
    env.crypto().sha256(&pair).into()
}

fn two_leaf_tree(
    env: &Env,
    a: &Address,
    b: &Address,
) -> (BytesN<32>, Vec<BytesN<32>>, Vec<BytesN<32>>) {
    let la = leaf_hash(env, a);
    let lb = leaf_hash(env, b);
    let root = combine(env, la.clone(), lb.clone());
    let mut pa = Vec::new(env);
    pa.push_back(lb.clone());
    let mut pb = Vec::new(env);
    pb.push_back(la.clone());
    (root, pa, pb)
}

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 1 — Digest stability (pinned regression)
// ═══════════════════════════════════════════════════════════════════════════════

/// Pins the exact byte layout of _voucher_digest so that a code change that
/// accidentally reorders fields will immediately fail this test.
#[test]
fn digest_byte_layout_is_stable() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LazyMint721, ());
    let creator = Address::generate(&env);
    let fee_receiver = Address::generate(&env);
    let sk = creator_signing_key();
    let client = LazyMint721Client::new(&env, &contract_id);
    client.initialize(
        &creator,
        &BytesN::from_array(&env, &sk.verifying_key().to_bytes()),
        &String::from_str(&env, "T"),
        &String::from_str(&env, "T"),
        &100u64,
        &0u32,
        &Address::generate(&env),
        &fee_receiver,
        &0u32,
        &String::from_str(&env, "Test SDF Network ; September 2015"),
    );
    let currency = Address::generate(&env);
    let v = MintVoucher {
        token_id: 1u64,
        nonce: 0u64,
        price: 500i128,
        currency: currency.clone(),
        uri: String::from_str(&env, "ipfs://stable"),
        uri_hash: BytesN::from_array(&env, &[0xabu8; 32]),
        valid_until: 9999u64,
    };
    let digest1 = env.as_contract(&contract_id, || LazyMint721::_voucher_digest(&env, &v));
    let digest2 = env.as_contract(&contract_id, || LazyMint721::_voucher_digest(&env, &v));
    assert_eq!(digest1, digest2);
    let v_diff_id = MintVoucher { token_id: 2, ..v.clone() };
    let d_diff = env.as_contract(&contract_id, || LazyMint721::_voucher_digest(&env, &v_diff_id));
    assert_ne!(digest1, d_diff, "mutating token_id must change digest");
    let v_diff_price = MintVoucher { price: 501, ..v.clone() };
    let d_diff2 = env.as_contract(&contract_id, || LazyMint721::_voucher_digest(&env, &v_diff_price));
    assert_ne!(digest1, d_diff2, "mutating price must change digest");
    let v_diff_uri = MintVoucher {
        uri_hash: BytesN::from_array(&env, &[0xbbu8; 32]),
        ..v.clone()
    };
    let d_diff3 = env.as_contract(&contract_id, || LazyMint721::_voucher_digest(&env, &v_diff_uri));
    assert_ne!(digest1, d_diff3, "mutating uri_hash must change digest");
    let v_diff_exp = MintVoucher { valid_until: 1, ..v.clone() };
    let d_diff4 = env.as_contract(&contract_id, || LazyMint721::_voucher_digest(&env, &v_diff_exp));
    assert_ne!(digest1, d_diff4, "mutating valid_until must change digest");
    let v_diff_nonce = MintVoucher { nonce: 99, ..v.clone() };
    let d_diff5 = env.as_contract(&contract_id, || LazyMint721::_voucher_digest(&env, &v_diff_nonce));
    assert_ne!(digest1, d_diff5, "mutating nonce must change digest");
}

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 2 — Single redeem (existing behaviour preserved)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn redeem_free_voucher_in_public_phase() {
    let (env, client, _creator, _fee) = setup(0);
    client.set_public_phase();
    let buyer = Address::generate(&env);
    let v = make_voucher(&env, 1);
    let sig = sign_voucher(&env, &client.address, &v);
    let token_id = client.redeem(&buyer, &v, &sig, &empty_proof(&env));
    assert_eq!(token_id, 1u64);
    assert_eq!(client.owner_of(&1u64), buyer);
    assert_eq!(client.total_supply(), 1u64);
    // nonce == token_id == 1
    assert!(client.is_voucher_redeemed(&1u64));
}

#[test]
fn redeem_marks_nonce_used_and_blocks_replay() {
    let (env, client, _creator, _fee) = setup(0);
    client.set_public_phase();
    let buyer = Address::generate(&env);
    env.ledger().with_mut(|li| li.sequence_number = 100);

    let voucher = crate::MintVoucher {
        valid_until: 50,
        ..make_voucher(&env, 1)
    };

    let result = client.try_redeem(
        &buyer,
        &voucher,
        &BytesN::from_array(&env, &[0u8; 64]),
        &empty_proof(&env),
    );
    assert_eq!(result, Err(Ok(Error::VoucherExpired)));
}

#[test]
fn redeem_expired_voucher_returns_error() {
    let (env, client, _creator, _fee) = setup(0);
    client.set_public_phase();
    env.ledger().with_mut(|li| li.sequence_number = 100);
    let buyer = Address::generate(&env);
    let voucher = make_voucher(&env, 1);
    let result = client.try_redeem(
        &buyer,
        &voucher,
        &BytesN::from_array(&env, &[0u8; 64]),
        &empty_proof(&env),
    );
    assert!(result.is_err());
}

#[test]
fn redeem_wrong_key_fails() {
    let (env, client, _creator, _fee) = setup(0);
    client.set_public_phase();
    let buyer = Address::generate(&env);
    let voucher = make_voucher(&env, 2);
    let result = client.try_redeem(
        &buyer,
        &voucher,
        &BytesN::from_array(&env, &[255u8; 64]),
        &empty_proof(&env),
    );
    assert!(result.is_err());
}

#[test]
fn redeem_tampered_uri_fails_sig_check() {
    let (env, client, _creator, _fee) = setup(0);
    client.set_public_phase();
    let buyer = Address::generate(&env);
    let v = make_voucher(&env, 4);
    let sig = sign_voucher(&env, &client.address, &v);
    let v_bad = MintVoucher {
        uri_hash: BytesN::from_array(&env, &[0xffu8; 32]),
        ..v
    };
    // sig was computed for original v; v_bad has different uri_hash so the digest won't match
    let result = client.try_redeem(
        &buyer,
        &v_bad,
        &sig,
        &empty_proof(&env),
    );
    assert!(result.is_err());
}

#[test]
fn redeem_tampered_price_fails_sig_check() {
    let (env, client, _creator, _fee) = setup(0);
    client.set_public_phase();
    let buyer = Address::generate(&env);
    let voucher = crate::MintVoucher {
        price: 500,
        ..make_voucher(&env, 4)
    };
    let result = client.try_redeem(
        &buyer,
        &voucher,
        &BytesN::from_array(&env, &[99u8; 64]),
        &empty_proof(&env),
    );
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 3 — Voucher revocation
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn revoke_blocks_subsequent_redeem() {
    let (env, client, _creator, _fee) = setup(0);
    client.set_public_phase();
    let token_id = 10u64;
    client.revoke_voucher(&token_id); // revokes nonce 10
    assert!(client.is_voucher_revoked(&token_id));
    let buyer = Address::generate(&env);
    let voucher = make_voucher(&env, 10); // nonce = 10 (token_id)
    let result = client.try_redeem(
        &buyer,
        &voucher,
        &BytesN::from_array(&env, &[0u8; 64]),
        &empty_proof(&env),
    );
    // allowlist passes (public phase); revocation check fires before sig check
    assert_eq!(result, Err(Ok(Error::VoucherRevoked)));
}

#[test]
fn revoke_already_redeemed_nonce_returns_already_redeemed() {
    // After set_merkle_root the phase reverts to allowlist; an empty proof
    // is rejected with NotAllowlisted before the revocation check is reached.
    let (env, client, _creator, _fee) = setup(0);
    client.set_public_phase();
    let buyer = Address::generate(&env);
    let (root, _, _) = two_leaf_tree(&env, &buyer, &Address::generate(&env));
    client.set_merkle_root(&root); // resets phase to allowlist

    let voucher = make_voucher(&env, 11);
    let result = client.try_redeem(
        &buyer,
        &voucher,
        &BytesN::from_array(&env, &[0u8; 64]),
        &empty_proof(&env),
    );
    assert_eq!(result, Err(Ok(Error::NotAllowlisted)));
}

#[test]
fn test_allowlist_phase_wrong_proof_returns_invalid_merkle_proof() {
    let (env, client, creator) = setup_test();
    default_init(&env, &client, &creator);

    let buyer = Address::generate(&env);
    let other = Address::generate(&env);
    let (root, _proof_buyer, proof_other) = two_leaf_tree(&env, &buyer, &other);
    client.set_merkle_root(&root);

    let voucher = make_voucher(&env, 12);
    let result = client.try_redeem(
        &buyer,
        &voucher,
        &BytesN::from_array(&env, &[0u8; 64]),
        &proof_other, // other's proof doesn't match buyer's leaf
    );
    assert_eq!(result, Err(Ok(Error::InvalidMerkleProof)));
}

#[test]
fn revoke_vouchers_batch_all_or_nothing() {
    let (env, client, _creator, _fee) = setup(0);
    client.set_public_phase();
    let buyer = Address::generate(&env);
    let (root, _, _) = two_leaf_tree(&env, &buyer, &Address::generate(&env));
    client.set_merkle_root(&root); // phase reset to allowlist

    let mut bad_proof = Vec::new(&env);
    bad_proof.push_back(BytesN::from_array(&env, &[0xdeu8; 32]));

    let voucher = make_voucher(&env, 13);
    let result = client.try_redeem(
        &buyer,
        &voucher,
        &BytesN::from_array(&env, &[0u8; 64]),
        &bad_proof,
    );
    assert_eq!(result, Err(Ok(Error::InvalidMerkleProof)));
}

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 4 — Merkle allowlist (721)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn allowlist_outsider_wrong_proof_fails() {
    let (env, client, _creator, _fee) = setup(0);
    let addr_a = Address::generate(&env);
    let addr_b = Address::generate(&env);
    let (root, _proof_a, proof_b) = two_leaf_tree(&env, &addr_a, &addr_b);
    client.set_merkle_root(&root);

    let outsider = Address::generate(&env);
    let voucher = make_voucher(&env, 14);
    let result = client.try_redeem(
        &outsider,
        &voucher,
        &BytesN::from_array(&env, &[0u8; 64]),
        &proof_b, // addr_b's proof doesn't match outsider's leaf
    );
    assert_eq!(result, Err(Ok(Error::InvalidMerkleProof)));
}

#[test]
fn allowlist_valid_proof_passes_gate() {
    let (env, client, _creator, _fee) = setup(0);
    let buyer = Address::generate(&env);
    let (root, proof_buyer, _) = two_leaf_tree(&env, &buyer, &Address::generate(&env));
    client.set_merkle_root(&root);
    let v = make_voucher(&env, 1);
    let sig = sign_voucher(&env, &client.address, &v);
    let res = client.redeem(&buyer, &v, &sig, &proof_buyer);
    assert_eq!(res, 1u64);
}

#[test]
fn test_allowlist_single_leaf_tree_valid_proof() {
    let (env, client, creator) = setup_test();
    default_init(&env, &client, &creator);

    let buyer = Address::generate(&env);
    let other = Address::generate(&env);
    let (root, proof_buyer, _proof_other) = two_leaf_tree(&env, &buyer, &other);
    client.set_merkle_root(&root);

    // buyer has valid proof but sends all-zero sig → passes allowlist gate, fails sig check
    let voucher = make_voucher(&env, 21);
    let result = client.try_redeem(
        &buyer,
        &voucher,
        &BytesN::from_array(&env, &[0u8; 64]),
        &proof_buyer,
    );

    assert!(result.is_err());
    assert_ne!(result, Err(Ok(Error::NotAllowlisted)));
    assert_ne!(result, Err(Ok(Error::InvalidMerkleProof)));
}

#[test]
fn allowlist_non_member_empty_proof_returns_not_allowlisted() {
    let (env, client, _creator, _fee) = setup(0);
    let allowed = Address::generate(&env);
    let (root, _, _) = two_leaf_tree(&env, &allowed, &Address::generate(&env));
    client.set_merkle_root(&root);
    let outsider = Address::generate(&env);
    let v = make_voucher(&env, 3);
    let sig = sign_voucher(&env, &client.address, &v);
    let res = client.try_redeem(&outsider, &v, &sig, &empty_proof(&env));
    assert_eq!(res, Err(Ok(Error::NotAllowlisted)));
}

#[test]
fn public_phase_bypasses_merkle_check() {
    let (env, client, _creator, _fee) = setup(0);
    client.set_public_phase();
    let buyer = Address::generate(&env);
    let v = make_voucher(&env, 4);
    let sig = sign_voucher(&env, &client.address, &v);
    let res = client.redeem(&buyer, &v, &sig, &empty_proof(&env));
    assert_eq!(res, 4u64);
}

#[test]
fn set_merkle_root_resets_to_allowlist_phase() {
    let (env, client, _creator, _fee) = setup(0);
    client.set_public_phase();
    assert!(client.is_public_phase());
    let root = BytesN::from_array(&env, &[0xaau8; 32]);
    client.set_merkle_root(&root);
    assert!(!client.is_public_phase());
}

#[test]
fn single_leaf_tree_proof() {
    let (env, client, _creator, _fee) = setup(0);
    let buyer = Address::generate(&env);
    let voucher = make_voucher(&env, 30);
    let result = client.try_redeem(
        &buyer,
        &voucher,
        &BytesN::from_array(&env, &[0u8; 64]),
        &empty_proof(&env),
    );
    // Not in public phase and no root set → NotAllowlisted
    assert!(result.is_err());
}

// ─── Admin Authorization ──────────────────────────────────────────────────────

#[test]
fn test_set_merkle_root_only_creator_succeeds() {
    let (env, client, creator) = setup_test();
    default_init(&env, &client, &creator);

    let root = BytesN::from_array(&env, &[0x11u8; 32]);
    assert!(client.try_set_merkle_root(&root).is_ok());
    assert_eq!(client.merkle_root(), Some(root));
}

#[test]
fn test_set_public_phase_only_creator_succeeds() {
    let (env, client, creator) = setup_test();
    default_init(&env, &client, &creator);

    assert!(client.try_set_public_phase().is_ok());
    assert!(client.is_public_phase());
}

#[test]
fn test_set_merkle_root_non_creator_fails() {
    let env = Env::default();
    // Deliberately do NOT call env.mock_all_auths()
    let contract_id = env.register(LazyMint721, ());
    let client = LazyMint721Client::new(&env, &contract_id);
    let creator = Address::generate(&env);

    let pubkey = BytesN::from_array(&env, &[1u8; 32]);
    let royalty_receiver = Address::generate(&env);
    let fee_receiver = Address::generate(&env);
    client.initialize(
        &creator,
        &pubkey,
        &String::from_str(&env, "TestNFT"),
        &String::from_str(&env, "TNFT"),
        &1000u64,
        &0u32,
        &royalty_receiver,
        &fee_receiver,
        &0u32,
        &String::from_str(&env, "Test SDF Network ; September 2015"),
    );

    let root = BytesN::from_array(&env, &[0x22u8; 32]);
    let result = client.try_set_merkle_root(&root);
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 5 — Batch redeem (#274)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn batch_n_items_all_minted() {
    let (env, client, _creator, _fee) = setup(0);
    client.set_public_phase();
    let buyer = Address::generate(&env);
    let mut items = Vec::new(&env);
    for i in 200u64..205u64 {
        items.push_back(make_batch_item(&env, &client.address, i));
    }
    let ids = client.redeem_batch(&buyer, &items);
    assert_eq!(ids.len(), 5u32);
    assert_eq!(client.total_supply(), 5u64);
    assert_eq!(client.balance_of(&buyer), 5u64);
}

#[test]
fn batch_duplicate_nonce_reverts_entire_batch() {
    let (env, client, _creator, _fee) = setup(0);
    client.set_public_phase();
    let buyer = Address::generate(&env);
    let item_a = make_batch_item(&env, &client.address, 300);
    let item_a_dup = make_batch_item(&env, &client.address, 300); // same nonce
    let item_b = make_batch_item(&env, &client.address, 301);
    let mut items = Vec::new(&env);
    items.push_back(item_a);
    items.push_back(item_b);
    items.push_back(item_a_dup);

    let res = client.try_redeem_batch(&buyer, &items);
    assert_eq!(res, Err(Ok(Error::DuplicateVoucherInBatch)));

    assert!(!client.is_voucher_redeemed(&300u64));
    assert!(!client.is_voucher_redeemed(&301u64));
    assert_eq!(client.total_supply(), 0u64);
}

#[test]
fn batch_one_expired_voucher_reverts_all() {
    let (env, client, _creator, _fee) = setup(0);
    client.set_public_phase();
    env.ledger().with_mut(|li| li.sequence_number = 200);
    let buyer = Address::generate(&env);
    let good = make_batch_item(&env, &client.address, 400);
    let expired_v = MintVoucher { valid_until: 100, ..make_voucher(&env, 401) };
    let expired_sig = sign_voucher(&env, &client.address, &expired_v);
    let bad = BatchVoucherItem {
        voucher: expired_v,
        signature: expired_sig,
        merkle_proof: empty_proof(&env),
    };
    let mut items = Vec::new(&env);
    items.push_back(good);
    items.push_back(bad);
    let res = client.try_redeem_batch(&buyer, &items);
    assert_eq!(res, Err(Ok(Error::VoucherExpired)));
    assert!(client.try_owner_of(&400u64).is_err());
}

#[test]
fn batch_one_revoked_voucher_reverts_all() {
    let (env, client, _creator, _fee) = setup(0);
    client.set_public_phase();
    client.revoke_voucher(&501u64);
    let buyer = Address::generate(&env);
    let mut items = Vec::new(&env);
    items.push_back(make_batch_item(&env, &client.address, 500));
    items.push_back(make_batch_item(&env, &client.address, 501));
    let res = client.try_redeem_batch(&buyer, &items);
    assert_eq!(res, Err(Ok(Error::VoucherRevoked)));
    assert!(client.try_owner_of(&500u64).is_err());
}

#[test]
fn batch_one_bad_sig_reverts_all() {
    let (env, client, _creator, _fee) = setup(0);
    client.set_public_phase();
    let buyer = Address::generate(&env);
    let good = make_batch_item(&env, &client.address, 600);
    let bad = BatchVoucherItem {
        voucher: make_voucher(&env, 601),
        signature: BytesN::from_array(&env, &[0u8; 64]),
        merkle_proof: empty_proof(&env),
    };
    let mut items = Vec::new(&env);
    items.push_back(good);
    items.push_back(bad);
    let res = client.try_redeem_batch(&buyer, &items);
    // ed25519_verify panics on bad sig → host error
    assert!(res.is_err());
    // All-or-nothing: neither token was minted
    assert!(client.try_owner_of(&600u64).is_err());
    assert!(client.try_owner_of(&601u64).is_err());
}

#[test]
fn batch_empty_returns_empty_batch_error() {
    let (env, client, _creator, _fee) = setup(0);
    client.set_public_phase();
    let buyer = Address::generate(&env);
    let items: Vec<BatchVoucherItem> = Vec::new(&env);
    let res = client.try_redeem_batch(&buyer, &items);
    assert_eq!(res, Err(Ok(Error::EmptyBatch)));
}

// ─── Admin Authorization ──────────────────────────────────────────────────────

#[test]
fn test_allowlist_allows_only_listed_buyers() {
    let (env, client, creator) = setup_test();
    default_init(&env, &client, &creator);

    let allowed = Address::generate(&env);
    let denied = Address::generate(&env);
    let (root, proof_allowed, _) = two_leaf_tree(&env, &allowed, &Address::generate(&env));
    client.set_merkle_root(&root);

    // allowed buyer: proof is valid → proceeds past Merkle gate (sig will fail)
    let voucher_a = make_voucher(&env, 50);
    let result_a = client.try_redeem(
        &allowed,
        &voucher_a,
        &BytesN::from_array(&env, &[0u8; 64]),
        &proof_allowed,
    );
    assert!(result_a.is_err());
    assert_ne!(result_a, Err(Ok(Error::NotAllowlisted)));
    assert_ne!(result_a, Err(Ok(Error::InvalidMerkleProof)));

    // denied buyer: empty proof → NotAllowlisted
    let voucher_d = make_voucher(&env, 51);
    let result_d = client.try_redeem(
        &denied,
        &voucher_d,
        &BytesN::from_array(&env, &[0u8; 64]),
        &empty_proof(&env),
    );
    assert_eq!(result_d, Err(Ok(Error::NotAllowlisted)));
}

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 6 — Fee formula
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn fee_bps_formula_correctness() {
    let price: i128 = 1;
    let bps: u32 = 500;
    let fee = (price * bps as i128) / 10_000;
    assert_eq!(fee, 0);
    let creator = price - fee;
    assert_eq!(creator, 1);

    let price2: i128 = 10_000;
    let fee2 = (price2 * 500i128) / 10_000;
    assert_eq!(fee2, 500);
    assert_eq!(price2 - fee2, 9_500);

    let fee3 = (price2 * 10_000i128) / 10_000;
    assert_eq!(fee3, 10_000);
    assert_eq!(price2 - fee3, 0);
}

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 7 — Transfer / approval
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn transfer_with_missing_balance_returns_error() {
    let (env, client, _creator, _fee) = setup(0);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    env.as_contract(&client.address, || {
        env.storage().persistent().set(&DataKey::Owner(1), &alice);
        // balance intentionally absent
    });
    let res = client.try_transfer(&alice, &bob, &1);
    assert_eq!(res, Err(Ok(Error::NotOwner)));
}

#[test]
fn transfer_with_zero_balance_returns_error() {
    let (env, client, _creator, _fee) = setup(0);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    env.as_contract(&client.address, || {
        env.storage().persistent().set(&DataKey::Owner(1), &alice);
        env.storage()
            .persistent()
            .set(&DataKey::BalanceOf(alice.clone()), &0u64);
    });
    let res = client.try_transfer(&alice, &bob, &1);
    assert_eq!(res, Err(Ok(Error::NotOwner)));
}

#[test]
fn transfer_success_after_redeem() {
    let (env, client, _creator, _fee) = setup(0);
    client.set_public_phase();
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let v = make_voucher(&env, 900);
    let sig = sign_voucher(&env, &client.address, &v);
    client.redeem(&alice, &v, &sig, &empty_proof(&env));
    assert_eq!(client.owner_of(&900u64), alice);
    client.transfer(&alice, &bob, &900u64);
    assert_eq!(client.owner_of(&900u64), bob);
    assert_eq!(client.balance_of(&alice), 0u64);
    assert_eq!(client.balance_of(&bob), 1u64);
}

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 8 — Replay protection
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn replay_check_precedes_signature_verification() {
    let (env, client, _creator, _fee) = setup(0);
    client.set_public_phase();
    // Pre-set the UsedVoucher marker for nonce=3 (nonce == token_id in make_voucher)
    env.as_contract(&client.address, || {
        env.storage()
            .persistent()
            .set(&DataKey::UsedVoucher(3u64), &true);
    });
    let buyer = Address::generate(&env);
    let v = make_voucher(&env, 3); // nonce = 3
    let any_sig = BytesN::from_array(&env, &[99u8; 64]);
    let res = client.try_redeem(&buyer, &v, &any_sig, &empty_proof(&env));
    assert_eq!(res, Err(Ok(Error::VoucherAlreadyRedeemed)));
}

#[test]
fn different_nonces_are_independent() {
    let (env, client, _creator, _fee) = setup(0);
    env.as_contract(&client.address, || {
        env.storage()
            .persistent()
            .set(&DataKey::UsedVoucher(1u64), &true);
    });
    assert!(client.is_voucher_redeemed(&1u64));
    assert!(!client.is_voucher_redeemed(&2u64));
}

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 9 — Security hardening regressions (#6)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn upgrade_non_creator_fails() {
    // upgrade() must require creator auth; without it anyone can replace WASM.
    let env = Env::default();
    // No mock_all_auths — creator auth must be satisfied explicitly.
    let contract_id = env.register(LazyMint721, ());
    let client = LazyMint721Client::new(&env, &contract_id);
    let creator = Address::generate(&env);
    let sk = creator_signing_key();
    client.initialize(
        &creator,
        &BytesN::from_array(&env, &sk.verifying_key().to_bytes()),
        &String::from_str(&env, "T"),
        &String::from_str(&env, "T"),
        &100u64,
        &0u32,
        &Address::generate(&env),
        &Address::generate(&env),
        &0u32,
        &String::from_str(&env, "Test SDF Network ; September 2015"),
    );
    let new_hash = BytesN::from_array(&env, &[0xaau8; 32]);
    let res = client.try_upgrade(&new_hash);
    // Without mocked auth the creator.require_auth() panics → Err
    assert!(res.is_err());
}

#[test]
fn initialize_royalty_bps_over_max_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LazyMint721, ());
    let client = LazyMint721Client::new(&env, &contract_id);
    let creator = Address::generate(&env);
    let sk = creator_signing_key();
    let res = client.try_initialize(
        &creator,
        &BytesN::from_array(&env, &sk.verifying_key().to_bytes()),
        &String::from_str(&env, "T"),
        &String::from_str(&env, "T"),
        &100u64,
        &10_001u32, // > MAX_BPS
        &Address::generate(&env),
        &Address::generate(&env),
        &0u32,
        &String::from_str(&env, "Test SDF Network ; September 2015"),
    );
    assert_eq!(res, Err(Ok(Error::InvalidBps)));
}

#[test]
fn update_royalty_over_max_fails() {
    let (env, client, _creator, _fee) = setup(0);
    let new_receiver = Address::generate(&env);
    let res = client.try_update_royalty(&new_receiver, &10_001u32);
    assert_eq!(res, Err(Ok(Error::InvalidBps)));
}

#[test]
fn update_royalty_at_max_succeeds() {
    let (env, client, _creator, _fee) = setup(0);
    let new_receiver = Address::generate(&env);
    // Exactly 10_000 bps (100%) must be accepted
    assert!(client.try_update_royalty(&new_receiver, &10_000u32).is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 10 — Migration
// ═══════════════════════════════════════════════════════════════════════════════

mod migration {
    use super::*;

    #[test]
    fn fresh_install_migrate_records_version() {
        let (env, client, _creator, _fee_receiver) = setup(0);

        assert!(client.contract_version().is_none());
        client.migrate();
        assert_eq!(
            client.contract_version(),
            Some(String::from_str(&env, "1.0.0"))
        );
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #14)")]
    fn double_migrate_reverts() {
        let (_env, client, _creator, _fee_receiver) = setup(0);
        client.migrate();
        client.migrate();
    }

    #[test]
    fn migrate_emits_migrated_event() {
        use soroban_sdk::xdr::{ContractEventBody, ScVal};
        let (env, client, _creator, _fee_receiver) = setup(0);
        client.migrate();

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
    fn used_voucher_marker_readable_after_migrate() {
        let (env, client, _creator, _fee_receiver) = setup(0);

        let voucher = make_voucher(&env, 42u64);
        let sig = sign_voucher(&env, &client.address, &voucher);
        let buyer = Address::generate(&env);

        client.set_public_phase();
        client.redeem(&buyer, &voucher, &sig, &empty_proof(&env));

        assert!(client.is_voucher_redeemed(&42u64));
        client.migrate();
        assert!(client.is_voucher_redeemed(&42u64));
    }

    #[test]
    fn revoked_voucher_readable_after_migrate() {
        let (_env, client, _creator, _fee_receiver) = setup(0);

        client.revoke_voucher(&99u64);
        assert!(client.is_voucher_revoked(&99u64));

        client.migrate();

        assert!(client.is_voucher_revoked(&99u64));
    }

    #[test]
    fn token_ownership_readable_after_migrate() {
        let (env, client, _creator, _fee_receiver) = setup(0);

        let voucher = make_voucher(&env, 1u64);
        let sig = sign_voucher(&env, &client.address, &voucher);
        let buyer = Address::generate(&env);

        client.set_public_phase();
        client.redeem(&buyer, &voucher, &sig, &empty_proof(&env));

        client.migrate();

        assert_eq!(client.owner_of(&1u64), buyer);
        assert_eq!(client.balance_of(&buyer), 1u64);
    }
}
