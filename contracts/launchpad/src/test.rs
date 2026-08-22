extern crate std;

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _, Ledger as _},
    token::{StellarAssetClient, TokenClient},
    xdr,
    xdr::ToXdr,
    Address, Bytes, BytesN, Env, IntoVal, String, TryFromVal, Val,
};

use crate::{CollectionKind, Error, Launchpad, LaunchpadClient};

const TEST_PASSPHRASE: &str = "Test SDF Network ; September 2015";

fn jump_ledger(env: &Env, delta: u32) {
    env.ledger().with_mut(|li| {
        li.sequence_number += delta;
    });
}

fn wasm_bytes(name: &str) -> std::vec::Vec<u8> {
    // In Cursor's sandbox, cargo builds into an isolated target dir (not `./target`).
    // Derive the target dir from the current test binary path:
    //   .../cargo-target/debug/deps/<test-binary>
    let exe = std::env::current_exe().unwrap();
    let target_dir = exe
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .unwrap()
        .to_path_buf();
    let path = target_dir
        .join("wasm32v1-none")
        .join("release")
        .join(std::format!("{name}.wasm"));

    std::fs::read(&path).unwrap_or_else(|_| {
        panic!(
            "missing wasm at {}. build it first with: cargo build --target wasm32v1-none --release -p collection-nft-erc1155 -p lazy-mint-erc721 -p collection-nft-erc721 -p lazy-mint-erc1155",
            path.display()
        )
    })
}

fn setup_launchpad(env: &Env) -> (LaunchpadClient<'_>, Address, Address, Address) {
    env.mock_all_auths();

    let launchpad_id = env.register(Launchpad, ());
    let client = LaunchpadClient::new(env, &launchpad_id);

    let admin = Address::generate(env);
    let fee_receiver = Address::generate(env);
    let creator = Address::generate(env);

    client.initialize(&admin, &fee_receiver, &0i128);

    let wasm_normal_721_bytes = wasm_bytes("collection_nft_erc721");
    let wasm_normal_1155_bytes = wasm_bytes("collection_nft_erc1155");
    let wasm_lazy_721_bytes = wasm_bytes("lazy_mint_erc721");
    let wasm_lazy_1155_bytes = wasm_bytes("lazy_mint_erc1155");

    let wasm_normal_721 = env
        .deployer()
        .upload_contract_wasm(wasm_normal_721_bytes.as_slice());
    let wasm_normal_1155 = env
        .deployer()
        .upload_contract_wasm(wasm_normal_1155_bytes.as_slice());
    let wasm_lazy_721 = env
        .deployer()
        .upload_contract_wasm(wasm_lazy_721_bytes.as_slice());
    let wasm_lazy_1155 = env
        .deployer()
        .upload_contract_wasm(wasm_lazy_1155_bytes.as_slice());

    client.set_wasm_hashes(
        &wasm_normal_721,
        &wasm_normal_1155,
        &wasm_lazy_721,
        &wasm_lazy_1155,
    );

    (client, admin, fee_receiver, creator)
}

/// Registers a Stellar Asset Contract and mints `amount` to `holder`.
fn setup_token(env: &Env, holder: &Address, amount: i128) -> Address {
    let token_admin = Address::generate(env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    StellarAssetClient::new(env, &token).mint(holder, &amount);
    token
}

/// True if the last invocation emitted an event from `contract` with exactly
/// these topics and data.
fn event_present(env: &Env, contract: &Address, topics: soroban_sdk::Vec<Val>, data: Val) -> bool {
    let topics_xdr: std::vec::Vec<xdr::ScVal> = topics
        .iter()
        .map(|t| xdr::ScVal::try_from_val(env, &t).unwrap())
        .collect();
    let data_xdr = xdr::ScVal::try_from_val(env, &data).unwrap();
    env.events()
        .all()
        .filter_by_contract(contract)
        .events()
        .iter()
        .any(|e| {
            matches!(
                &e.body,
                xdr::ContractEventBody::V0(v0)
                    if v0.topics.as_slice() == topics_xdr.as_slice() && v0.data == data_xdr
            )
        })
}

/// True if the last invocation emitted any event from `contract` whose first
/// topic is `tag`.
fn event_with_tag_present(env: &Env, contract: &Address, tag: soroban_sdk::Symbol) -> bool {
    let tag_val: Val = tag.into_val(env);
    let tag_xdr = xdr::ScVal::try_from_val(env, &tag_val).unwrap();
    env.events()
        .all()
        .filter_by_contract(contract)
        .events()
        .iter()
        .any(|e| {
            matches!(
                &e.body,
                xdr::ContractEventBody::V0(v0) if v0.topics.first() == Some(&tag_xdr)
            )
        })
}

#[test]
fn deploys_normal_721_twice_with_unique_addresses() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, creator) = setup_launchpad(&env);

    let salt_a = BytesN::from_array(&env, &[10u8; 32]);
    let salt_b = BytesN::from_array(&env, &[11u8; 32]);
    let royalty_receiver = Address::generate(&env);
    let currency = Address::generate(&env);

    let deployed_a = client.deploy_normal_721(
        &creator,
        &currency,
        &String::from_str(&env, "Creator 721 A"),
        &String::from_str(&env, "C721A"),
        &1_000u64,
        &500u32,
        &royalty_receiver,
        &0u32,
        &salt_a,
    );

    let deployed_b = client.deploy_normal_721(
        &creator,
        &currency,
        &String::from_str(&env, "Creator 721 B"),
        &String::from_str(&env, "C721B"),
        &1_500u64,
        &500u32,
        &royalty_receiver,
        &0u32,
        &salt_b,
    );

    assert_ne!(deployed_a, deployed_b);
    assert_eq!(client.collection_count(), 2u64);

    let all = client.all_collections();
    assert_eq!(all.len(), 2);
    assert!(matches!(
        all.get(0).unwrap().kind,
        CollectionKind::Normal721
    ));
    assert!(matches!(
        all.get(1).unwrap().kind,
        CollectionKind::Normal721
    ));
}

#[test]
fn deploys_normal_1155_twice_with_unique_addresses() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, creator) = setup_launchpad(&env);

    let salt_a = BytesN::from_array(&env, &[20u8; 32]);
    let salt_b = BytesN::from_array(&env, &[21u8; 32]);
    let royalty_receiver = Address::generate(&env);
    let currency = Address::generate(&env);

    let deployed_a = client.deploy_normal_1155(
        &creator,
        &currency,
        &String::from_str(&env, "Creator 1155 A"),
        &500u32,
        &royalty_receiver,
        &0u32,
        &salt_a,
    );

    let deployed_b = client.deploy_normal_1155(
        &creator,
        &currency,
        &String::from_str(&env, "Creator 1155 B"),
        &500u32,
        &royalty_receiver,
        &0u32,
        &salt_b,
    );

    assert_ne!(deployed_a, deployed_b);
    assert_eq!(client.collection_count(), 2u64);

    let all = client.all_collections();
    assert_eq!(all.len(), 2);
    assert!(matches!(
        all.get(0).unwrap().kind,
        CollectionKind::Normal1155
    ));
    assert!(matches!(
        all.get(1).unwrap().kind,
        CollectionKind::Normal1155
    ));
}

#[test]
fn deploys_lazy_721_twice_with_unique_addresses() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, creator) = setup_launchpad(&env);

    let salt_a = BytesN::from_array(&env, &[30u8; 32]);
    let salt_b = BytesN::from_array(&env, &[31u8; 32]);
    let creator_pubkey = BytesN::from_array(&env, &[7u8; 32]);
    let royalty_receiver = Address::generate(&env);
    let currency = Address::generate(&env);

    let deployed_a = client.deploy_lazy_721(
        &creator,
        &currency,
        &creator_pubkey,
        &String::from_str(&env, "Lazy 721 A"),
        &String::from_str(&env, "LZ7A"),
        &1_000u64,
        &750u32,
        &royalty_receiver,
        &0u32,
        &salt_a,
        &String::from_str(&env, TEST_PASSPHRASE),
    );

    let deployed_b = client.deploy_lazy_721(
        &creator,
        &currency,
        &creator_pubkey,
        &String::from_str(&env, "Lazy 721 B"),
        &String::from_str(&env, "LZ7B"),
        &1_200u64,
        &750u32,
        &royalty_receiver,
        &0u32,
        &salt_b,
        &String::from_str(&env, TEST_PASSPHRASE),
    );

    assert_ne!(deployed_a, deployed_b);
    assert_eq!(client.collection_count(), 2u64);

    let all = client.all_collections();
    assert_eq!(all.len(), 2);
    assert!(matches!(
        all.get(0).unwrap().kind,
        CollectionKind::LazyMint721
    ));
    assert!(matches!(
        all.get(1).unwrap().kind,
        CollectionKind::LazyMint721
    ));
}

#[test]
fn deploys_lazy_1155_twice_with_unique_addresses() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, creator) = setup_launchpad(&env);

    let salt_a = BytesN::from_array(&env, &[40u8; 32]);
    let salt_b = BytesN::from_array(&env, &[41u8; 32]);
    let creator_pubkey = BytesN::from_array(&env, &[9u8; 32]);
    let royalty_receiver = Address::generate(&env);
    let currency = Address::generate(&env);

    let deployed_a = client.deploy_lazy_1155(
        &creator,
        &currency,
        &creator_pubkey,
        &String::from_str(&env, "Lazy 1155 A"),
        &600u32,
        &royalty_receiver,
        &0u32,
        &salt_a,
        &String::from_str(&env, TEST_PASSPHRASE),
    );

    let deployed_b = client.deploy_lazy_1155(
        &creator,
        &currency,
        &creator_pubkey,
        &String::from_str(&env, "Lazy 1155 B"),
        &600u32,
        &royalty_receiver,
        &0u32,
        &salt_b,
        &String::from_str(&env, TEST_PASSPHRASE),
    );

    assert_ne!(deployed_a, deployed_b);
    assert_eq!(client.collection_count(), 2u64);

    let all = client.all_collections();
    assert_eq!(all.len(), 2);
    assert!(matches!(
        all.get(0).unwrap().kind,
        CollectionKind::LazyMint1155
    ));
    assert!(matches!(
        all.get(1).unwrap().kind,
        CollectionKind::LazyMint1155
    ));
}

#[test]
fn deploy_calls_extend_instance_ttl() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, creator) = setup_launchpad(&env);

    let royalty_receiver = Address::generate(&env);
    let currency = Address::generate(&env);

    // After initialize(), instance TTL is bumped to 100_000 ledgers.
    // Move forward so remaining TTL is below threshold (50_000),
    // then call deploy_* which should bump instance TTL again.
    jump_ledger(&env, 60_000);

    let salt_a = BytesN::from_array(&env, &[60u8; 32]);
    let _deployed_a = client.deploy_normal_721(
        &creator,
        &currency,
        &String::from_str(&env, "TTL A"),
        &String::from_str(&env, "TTLA"),
        &100u64,
        &500u32,
        &royalty_receiver,
        &0u32,
        &salt_a,
    );

    // Without TTL extension on deploy, instance storage would now be expired:
    // 60_000 + 60_000 > 100_000.
    jump_ledger(&env, 60_000);

    let salt_b = BytesN::from_array(&env, &[61u8; 32]);
    let _deployed_b = client.deploy_normal_1155(
        &creator,
        &currency,
        &String::from_str(&env, "TTL B"),
        &500u32,
        &royalty_receiver,
        &0u32,
        &salt_b,
    );

    assert_eq!(client.collection_count(), 2u64);
}

#[test]
fn admin_calls_extend_instance_ttl() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, _creator) = setup_launchpad(&env);

    jump_ledger(&env, 60_000);

    let new_admin = Address::generate(&env);
    client.transfer_admin(&new_admin);

    jump_ledger(&env, 60_000);

    client.accept_admin(&new_admin);
    assert_eq!(client.admin(), new_admin);
}

// ─── Issue #53 — Salt front-running / griefing tests ─────────────────────────
//
// The fix: secure_salt = sha256(creator.to_xdr() ‖ raw_salt)
//
// Two categories of tests:
//   A. Same raw salt from two different creators → different deployed addresses.
//   B. Front-runner copies Alice's raw salt and transacts first → Alice's
//      subsequent transaction still succeeds (different address).

// ── Category A: Per-creator namespace isolation ──────────────────────────────

/// deploy_normal_721: same raw salt, different creators ⟹ different addresses.
#[test]
fn same_salt_different_creators_normal_721_yields_different_addresses() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, alice) = setup_launchpad(&env);
    let bob = Address::generate(&env);

    let salt = BytesN::from_array(&env, &[0xAAu8; 32]);
    let royalty_receiver = Address::generate(&env);
    let currency = Address::generate(&env);

    let addr_alice = client.deploy_normal_721(
        &alice,
        &currency,
        &String::from_str(&env, "Alice 721"),
        &String::from_str(&env, "AL7"),
        &100u64,
        &500u32,
        &royalty_receiver,
        &0u32,
        &salt,
    );

    let addr_bob = client.deploy_normal_721(
        &bob,
        &currency,
        &String::from_str(&env, "Bob 721"),
        &String::from_str(&env, "BO7"),
        &100u64,
        &500u32,
        &royalty_receiver,
        &0u32,
        &salt, // identical raw salt
    );

    // Because secure_salt = sha256(creator ‖ raw_salt) they must differ.
    assert_ne!(
        addr_alice, addr_bob,
        "same raw salt must not collide across creators"
    );
    assert_eq!(client.collection_count(), 2u64);
}

/// deploy_normal_1155: same raw salt, different creators ⟹ different addresses.
#[test]
fn same_salt_different_creators_normal_1155_yields_different_addresses() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, alice) = setup_launchpad(&env);
    let bob = Address::generate(&env);

    let salt = BytesN::from_array(&env, &[0xBBu8; 32]);
    let royalty_receiver = Address::generate(&env);
    let currency = Address::generate(&env);

    let addr_alice = client.deploy_normal_1155(
        &alice,
        &currency,
        &String::from_str(&env, "Alice 1155"),
        &500u32,
        &royalty_receiver,
        &0u32,
        &salt,
    );

    let addr_bob = client.deploy_normal_1155(
        &bob,
        &currency,
        &String::from_str(&env, "Bob 1155"),
        &500u32,
        &royalty_receiver,
        &0u32,
        &salt,
    );

    assert_ne!(addr_alice, addr_bob);
    assert_eq!(client.collection_count(), 2u64);
}

/// deploy_lazy_721: same raw salt, different creators ⟹ different addresses.
#[test]
fn same_salt_different_creators_lazy_721_yields_different_addresses() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, alice) = setup_launchpad(&env);
    let bob = Address::generate(&env);

    let salt = BytesN::from_array(&env, &[0xCCu8; 32]);
    let creator_pubkey = BytesN::from_array(&env, &[0x01u8; 32]);
    let royalty_receiver = Address::generate(&env);
    let currency = Address::generate(&env);

    let addr_alice = client.deploy_lazy_721(
        &alice,
        &currency,
        &creator_pubkey,
        &String::from_str(&env, "Alice L721"),
        &String::from_str(&env, "AL7L"),
        &500u64,
        &300u32,
        &royalty_receiver,
        &0u32,
        &salt,
        &String::from_str(&env, TEST_PASSPHRASE),
    );

    let addr_bob = client.deploy_lazy_721(
        &bob,
        &currency,
        &creator_pubkey,
        &String::from_str(&env, "Bob L721"),
        &String::from_str(&env, "BO7L"),
        &500u64,
        &300u32,
        &royalty_receiver,
        &0u32,
        &salt,
        &String::from_str(&env, TEST_PASSPHRASE),
    );

    assert_ne!(addr_alice, addr_bob);
    assert_eq!(client.collection_count(), 2u64);
}

/// deploy_lazy_1155: same raw salt, different creators ⟹ different addresses.
#[test]
fn same_salt_different_creators_lazy_1155_yields_different_addresses() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, alice) = setup_launchpad(&env);
    let bob = Address::generate(&env);

    let salt = BytesN::from_array(&env, &[0xDDu8; 32]);
    let creator_pubkey = BytesN::from_array(&env, &[0x02u8; 32]);
    let royalty_receiver = Address::generate(&env);
    let currency = Address::generate(&env);

    let addr_alice = client.deploy_lazy_1155(
        &alice,
        &currency,
        &creator_pubkey,
        &String::from_str(&env, "Alice L1155"),
        &400u32,
        &royalty_receiver,
        &0u32,
        &salt,
        &String::from_str(&env, TEST_PASSPHRASE),
    );

    let addr_bob = client.deploy_lazy_1155(
        &bob,
        &currency,
        &creator_pubkey,
        &String::from_str(&env, "Bob L1155"),
        &400u32,
        &royalty_receiver,
        &0u32,
        &salt,
        &String::from_str(&env, TEST_PASSPHRASE),
    );

    assert_ne!(addr_alice, addr_bob);
    assert_eq!(client.collection_count(), 2u64);
}

// ── Category B: Front-runner cannot block the victim ─────────────────────────
//
// Bob front-runs with the same raw salt as Alice.  After the fix, Bob's
// deploy lands at sha256(Bob ‖ salt).  Alice's subsequent deploy lands at
// sha256(Alice ‖ salt) — a distinct address — so her tx must succeed.

/// deploy_normal_721: front-runner copies Alice's salt → Alice still succeeds.
#[test]
fn front_runner_cannot_grief_normal_721() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, alice) = setup_launchpad(&env);
    let bob = Address::generate(&env); // malicious actor

    let salt = BytesN::from_array(&env, &[0x11u8; 32]);
    let royalty_receiver = Address::generate(&env);
    let currency = Address::generate(&env);

    // Bob front-runs using Alice's raw salt.
    let addr_bob = client.deploy_normal_721(
        &bob,
        &currency,
        &String::from_str(&env, "Bob Grief 721"),
        &String::from_str(&env, "BG7"),
        &100u64,
        &0u32,
        &royalty_receiver,
        &0u32,
        &salt,
    );

    // Alice's transaction must still succeed (no panic / error).
    let addr_alice = client.deploy_normal_721(
        &alice,
        &currency,
        &String::from_str(&env, "Alice 721"),
        &String::from_str(&env, "AL7"),
        &100u64,
        &0u32,
        &royalty_receiver,
        &0u32,
        &salt,
    );

    assert_ne!(
        addr_alice, addr_bob,
        "front-runner must not occupy Alice's slot"
    );
    assert_eq!(client.collection_count(), 2u64);
}

/// deploy_normal_1155: front-runner copies Alice's salt → Alice still succeeds.
#[test]
fn front_runner_cannot_grief_normal_1155() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, alice) = setup_launchpad(&env);
    let bob = Address::generate(&env);

    let salt = BytesN::from_array(&env, &[0x22u8; 32]);
    let royalty_receiver = Address::generate(&env);
    let currency = Address::generate(&env);

    let addr_bob = client.deploy_normal_1155(
        &bob,
        &currency,
        &String::from_str(&env, "Bob Grief 1155"),
        &0u32,
        &royalty_receiver,
        &0u32,
        &salt,
    );

    let addr_alice = client.deploy_normal_1155(
        &alice,
        &currency,
        &String::from_str(&env, "Alice 1155"),
        &0u32,
        &royalty_receiver,
        &0u32,
        &salt,
    );

    assert_ne!(addr_alice, addr_bob);
    assert_eq!(client.collection_count(), 2u64);
}

/// deploy_lazy_721: front-runner copies Alice's salt → Alice still succeeds.
#[test]
fn front_runner_cannot_grief_lazy_721() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, alice) = setup_launchpad(&env);
    let bob = Address::generate(&env);

    let salt = BytesN::from_array(&env, &[0x33u8; 32]);
    let creator_pubkey = BytesN::from_array(&env, &[0x03u8; 32]);
    let royalty_receiver = Address::generate(&env);
    let currency = Address::generate(&env);

    let addr_bob = client.deploy_lazy_721(
        &bob,
        &currency,
        &creator_pubkey,
        &String::from_str(&env, "Bob Grief L721"),
        &String::from_str(&env, "BGL7"),
        &200u64,
        &0u32,
        &royalty_receiver,
        &0u32,
        &salt,
        &String::from_str(&env, TEST_PASSPHRASE),
    );

    let addr_alice = client.deploy_lazy_721(
        &alice,
        &currency,
        &creator_pubkey,
        &String::from_str(&env, "Alice L721"),
        &String::from_str(&env, "ALL7"),
        &200u64,
        &0u32,
        &royalty_receiver,
        &0u32,
        &salt,
        &String::from_str(&env, TEST_PASSPHRASE),
    );

    assert_ne!(addr_alice, addr_bob);
    assert_eq!(client.collection_count(), 2u64);
}

/// deploy_lazy_1155: front-runner copies Alice's salt → Alice still succeeds.
#[test]
fn front_runner_cannot_grief_lazy_1155() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, alice) = setup_launchpad(&env);
    let bob = Address::generate(&env);

    let salt = BytesN::from_array(&env, &[0x44u8; 32]);
    let creator_pubkey = BytesN::from_array(&env, &[0x04u8; 32]);
    let royalty_receiver = Address::generate(&env);
    let currency = Address::generate(&env);

    let addr_bob = client.deploy_lazy_1155(
        &bob,
        &currency,
        &creator_pubkey,
        &String::from_str(&env, "Bob Grief L1155"),
        &0u32,
        &royalty_receiver,
        &0u32,
        &salt,
        &String::from_str(&env, TEST_PASSPHRASE),
    );

    let addr_alice = client.deploy_lazy_1155(
        &alice,
        &currency,
        &creator_pubkey,
        &String::from_str(&env, "Alice L1155"),
        &0u32,
        &royalty_receiver,
        &0u32,
        &salt,
        &String::from_str(&env, TEST_PASSPHRASE),
    );

    assert_ne!(addr_alice, addr_bob);
    assert_eq!(client.collection_count(), 2u64);
}

// ── Category C: Duplicate (creator, salt) deploy reverts cleanly ─────────────
//
// Deploying with the same creator AND same raw salt a second time must revert
// because the derived secure_salt (sha256(creator ‖ raw_salt)) is identical,
// so the factory would try to instantiate a contract at an already-occupied
// deterministic address — the Soroban VM rejects this with a host error.

/// deploy_normal_721: same creator, same salt → second deploy reverts.
#[test]
#[should_panic]
fn duplicate_creator_salt_normal_721_reverts() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, creator) = setup_launchpad(&env);
    let salt = BytesN::from_array(&env, &[0xE1u8; 32]);
    let royalty_receiver = Address::generate(&env);
    let currency = Address::generate(&env);

    client.deploy_normal_721(
        &creator,
        &currency,
        &String::from_str(&env, "First 721"),
        &String::from_str(&env, "F721"),
        &100u64,
        &0u32,
        &royalty_receiver,
        &0u32,
        &salt,
    );
    // Second call with identical creator + salt must panic.
    client.deploy_normal_721(
        &creator,
        &currency,
        &String::from_str(&env, "Dupe 721"),
        &String::from_str(&env, "D721"),
        &100u64,
        &0u32,
        &royalty_receiver,
        &0u32,
        &salt,
    );
}

/// deploy_normal_1155: same creator, same salt → second deploy reverts.
#[test]
#[should_panic]
fn duplicate_creator_salt_normal_1155_reverts() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, creator) = setup_launchpad(&env);
    let salt = BytesN::from_array(&env, &[0xE2u8; 32]);
    let royalty_receiver = Address::generate(&env);
    let currency = Address::generate(&env);

    client.deploy_normal_1155(
        &creator,
        &currency,
        &String::from_str(&env, "First 1155"),
        &0u32,
        &royalty_receiver,
        &0u32,
        &salt,
    );
    client.deploy_normal_1155(
        &creator,
        &currency,
        &String::from_str(&env, "Dupe 1155"),
        &0u32,
        &royalty_receiver,
        &0u32,
        &salt,
    );
}

/// deploy_lazy_721: same creator, same salt → second deploy reverts.
#[test]
#[should_panic]
fn duplicate_creator_salt_lazy_721_reverts() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, creator) = setup_launchpad(&env);
    let salt = BytesN::from_array(&env, &[0xE3u8; 32]);
    let creator_pubkey = BytesN::from_array(&env, &[0x01u8; 32]);
    let royalty_receiver = Address::generate(&env);
    let currency = Address::generate(&env);

    client.deploy_lazy_721(
        &creator,
        &currency,
        &creator_pubkey,
        &String::from_str(&env, "First L721"),
        &String::from_str(&env, "FL72"),
        &100u64,
        &0u32,
        &royalty_receiver,
        &0u32,
        &salt,
        &String::from_str(&env, TEST_PASSPHRASE),
    );
    client.deploy_lazy_721(
        &creator,
        &currency,
        &creator_pubkey,
        &String::from_str(&env, "Dupe L721"),
        &String::from_str(&env, "DL72"),
        &100u64,
        &0u32,
        &royalty_receiver,
        &0u32,
        &salt,
        &String::from_str(&env, TEST_PASSPHRASE),
    );
}

/// deploy_lazy_1155: same creator, same salt → second deploy reverts.
#[test]
#[should_panic]
fn duplicate_creator_salt_lazy_1155_reverts() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, creator) = setup_launchpad(&env);
    let salt = BytesN::from_array(&env, &[0xE4u8; 32]);
    let creator_pubkey = BytesN::from_array(&env, &[0x02u8; 32]);
    let royalty_receiver = Address::generate(&env);
    let currency = Address::generate(&env);

    client.deploy_lazy_1155(
        &creator,
        &currency,
        &creator_pubkey,
        &String::from_str(&env, "First L1155"),
        &0u32,
        &royalty_receiver,
        &0u32,
        &salt,
        &String::from_str(&env, TEST_PASSPHRASE),
    );
    client.deploy_lazy_1155(
        &creator,
        &currency,
        &creator_pubkey,
        &String::from_str(&env, "Dupe L1155"),
        &0u32,
        &royalty_receiver,
        &0u32,
        &salt,
        &String::from_str(&env, TEST_PASSPHRASE),
    );
}

// ── Deterministic address regression ────────────────────────────────────────
//
// The factory must keep deriving addresses from
// sha256(creator.to_xdr() ‖ raw_salt) so clients can pre-compute them.

#[test]
fn secure_salt_address_derivation_is_unchanged() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, creator) = setup_launchpad(&env);

    let salt = BytesN::from_array(&env, &[0x5Au8; 32]);

    // Recompute the address exactly as documented: the deployed address is
    // derived from the factory address and sha256(creator ‖ raw_salt).
    let mut raw = Bytes::new(&env);
    raw.append(&creator.clone().to_xdr(&env));
    raw.extend_from_array(&salt.to_array());
    let secure_salt: BytesN<32> = env.crypto().sha256(&raw).into();
    let expected = env
        .deployer()
        .with_address(client.address.clone(), secure_salt)
        .deployed_address();

    let royalty_receiver = Address::generate(&env);
    let currency = Address::generate(&env);
    let got = client.deploy_normal_721(
        &creator,
        &currency,
        &String::from_str(&env, "Deterministic"),
        &String::from_str(&env, "DET"),
        &100u64,
        &0u32,
        &royalty_receiver,
        &0u32,
        &salt,
    );

    assert_eq!(got, expected, "make_secure_salt derivation changed");
}

// ── Initialisation error tests ──────────────────────────────────

#[test]
fn initialize_twice_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let launchpad_id = env.register(Launchpad, ());
    let client = LaunchpadClient::new(&env, &launchpad_id);

    let admin = Address::generate(&env);
    let fee_receiver = Address::generate(&env);
    client.initialize(&admin, &fee_receiver, &0i128);

    let result = client.try_initialize(&admin, &fee_receiver, &0i128);
    assert_eq!(result, Err(Ok(Error::AlreadyInitialized)));
}

#[test]
fn initialize_rejects_negative_deploy_fee() {
    let env = Env::default();
    env.mock_all_auths();

    let launchpad_id = env.register(Launchpad, ());
    let client = LaunchpadClient::new(&env, &launchpad_id);

    let admin = Address::generate(&env);
    let fee_receiver = Address::generate(&env);
    let result = client.try_initialize(&admin, &fee_receiver, &-1i128);
    assert_eq!(result, Err(Ok(Error::InvalidDeployFee)));
}

#[test]
fn deploy_without_wasm_hashes_fails_for_all_kinds() {
    let env = Env::default();
    env.mock_all_auths();

    let launchpad_id = env.register(Launchpad, ());
    let client = LaunchpadClient::new(&env, &launchpad_id);

    let admin = Address::generate(&env);
    let fee_receiver = Address::generate(&env);
    let creator = Address::generate(&env);
    client.initialize(&admin, &fee_receiver, &0i128);

    let salt = BytesN::from_array(&env, &[0x99u8; 32]);
    let currency = Address::generate(&env);
    let royalty_receiver = Address::generate(&env);
    let creator_pubkey = BytesN::from_array(&env, &[0x05u8; 32]);

    let result = client.try_deploy_normal_721(
        &creator,
        &currency,
        &String::from_str(&env, "No Wasm"),
        &String::from_str(&env, "NOWASM"),
        &100u64,
        &500u32,
        &royalty_receiver,
        &0u32,
        &salt,
    );
    assert_eq!(result, Err(Ok(Error::WasmHashNotSet)));

    let result = client.try_deploy_normal_1155(
        &creator,
        &currency,
        &String::from_str(&env, "No Wasm"),
        &500u32,
        &royalty_receiver,
        &0u32,
        &salt,
    );
    assert_eq!(result, Err(Ok(Error::WasmHashNotSet)));

    let result = client.try_deploy_lazy_721(
        &creator,
        &currency,
        &creator_pubkey,
        &String::from_str(&env, "No Wasm"),
        &String::from_str(&env, "NOWASM"),
        &100u64,
        &500u32,
        &royalty_receiver,
        &0u32,
        &salt,
        &String::from_str(&env, TEST_PASSPHRASE),
    );
    assert_eq!(result, Err(Ok(Error::WasmHashNotSet)));

    let result = client.try_deploy_lazy_1155(
        &creator,
        &currency,
        &creator_pubkey,
        &String::from_str(&env, "No Wasm"),
        &500u32,
        &royalty_receiver,
        &0u32,
        &salt,
        &String::from_str(&env, TEST_PASSPHRASE),
    );
    assert_eq!(result, Err(Ok(Error::WasmHashNotSet)));
}

// ── Admin function tests ────────────────────────────────────────

#[test]
fn admin_calls_before_init_fail() {
    let env = Env::default();
    env.mock_all_auths();

    let launchpad_id = env.register(Launchpad, ());
    let client = LaunchpadClient::new(&env, &launchpad_id);

    let new_admin = Address::generate(&env);
    assert_eq!(
        client.try_transfer_admin(&new_admin),
        Err(Ok(Error::NotInitialized))
    );
    assert_eq!(
        client.try_set_fee_config(&Address::generate(&env), &100i128),
        Err(Ok(Error::NotInitialized))
    );
    assert_eq!(client.try_pause(), Err(Ok(Error::NotInitialized)));
    assert_eq!(client.try_unpause(), Err(Ok(Error::NotInitialized)));
    assert_eq!(
        client.try_cancel_admin_transfer(),
        Err(Ok(Error::NotInitialized))
    );
}

// ── Two-step admin transfer state machine ───────────────────────

#[test]
fn admin_transfer_propose_then_accept() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, admin, _fee_receiver, _creator) = setup_launchpad(&env);

    let new_admin = Address::generate(&env);
    client.transfer_admin(&new_admin);

    // Proposal event emitted; admin unchanged until acceptance.
    assert!(event_present(
        &env,
        &client.address,
        (symbol_short!("admin"), symbol_short!("proposed")).into_val(&env),
        (admin.clone(), new_admin.clone()).into_val(&env),
    ));
    assert_eq!(client.admin(), admin);
    assert_eq!(client.pending_admin(), Some(new_admin.clone()));

    client.accept_admin(&new_admin);
    assert!(event_present(
        &env,
        &client.address,
        (symbol_short!("admin"), symbol_short!("accepted")).into_val(&env),
        (admin.clone(), new_admin.clone()).into_val(&env),
    ));
    assert_eq!(client.admin(), new_admin);
    assert_eq!(client.pending_admin(), None);
}

#[test]
fn admin_transfer_repropose_overwrites_pending() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, _creator) = setup_launchpad(&env);

    let first = Address::generate(&env);
    let second = Address::generate(&env);

    client.transfer_admin(&first);
    client.transfer_admin(&second);
    assert_eq!(client.pending_admin(), Some(second.clone()));

    // The stale proposal can no longer be accepted.
    assert_eq!(
        client.try_accept_admin(&first),
        Err(Ok(Error::NotPendingAdmin))
    );

    client.accept_admin(&second);
    assert_eq!(client.admin(), second);
}

#[test]
fn accept_admin_by_wrong_address_fails() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, admin, _fee_receiver, _creator) = setup_launchpad(&env);

    let new_admin = Address::generate(&env);
    let interloper = Address::generate(&env);
    client.transfer_admin(&new_admin);

    assert_eq!(
        client.try_accept_admin(&interloper),
        Err(Ok(Error::NotPendingAdmin))
    );
    assert_eq!(client.admin(), admin);
    assert_eq!(client.pending_admin(), Some(new_admin));
}

#[test]
fn accept_admin_without_pending_fails() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, _creator) = setup_launchpad(&env);

    let someone = Address::generate(&env);
    assert_eq!(
        client.try_accept_admin(&someone),
        Err(Ok(Error::NoPendingAdmin))
    );
}

#[test]
fn cancel_admin_transfer_clears_pending() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, admin, _fee_receiver, _creator) = setup_launchpad(&env);

    let new_admin = Address::generate(&env);
    client.transfer_admin(&new_admin);
    client.cancel_admin_transfer();

    assert!(event_present(
        &env,
        &client.address,
        (symbol_short!("admin"), symbol_short!("cancelled")).into_val(&env),
        (admin.clone(), new_admin.clone()).into_val(&env),
    ));
    assert_eq!(client.pending_admin(), None);
    assert_eq!(
        client.try_accept_admin(&new_admin),
        Err(Ok(Error::NoPendingAdmin))
    );
}

#[test]
fn cancel_admin_transfer_without_pending_fails() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, _creator) = setup_launchpad(&env);

    assert_eq!(
        client.try_cancel_admin_transfer(),
        Err(Ok(Error::NoPendingAdmin))
    );
}

// ── Auth-failure tests: every admin function reverts without admin auth ──────

#[test]
fn admin_functions_revert_without_admin_auth() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, _creator) = setup_launchpad(&env);

    // Drop all mocked auths: the stored admin's require_auth() must now fail.
    env.set_auths(&[]);

    let hash = BytesN::from_array(&env, &[1u8; 32]);
    assert!(client
        .try_set_wasm_hashes(&hash, &hash, &hash, &hash)
        .is_err());
    assert!(client
        .try_set_fee_config(&Address::generate(&env), &10i128)
        .is_err());
    assert!(client.try_transfer_admin(&Address::generate(&env)).is_err());
    assert!(client.try_cancel_admin_transfer().is_err());
    assert!(client.try_pause().is_err());
    assert!(client.try_unpause().is_err());
}

#[test]
fn accept_admin_reverts_without_pending_admin_auth() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, admin, _fee_receiver, _creator) = setup_launchpad(&env);

    let new_admin = Address::generate(&env);
    client.transfer_admin(&new_admin);

    // Without the successor's signature, acceptance must fail.
    env.set_auths(&[]);
    assert!(client.try_accept_admin(&new_admin).is_err());

    env.mock_all_auths();
    assert_eq!(client.admin(), admin);
    assert_eq!(client.pending_admin(), Some(new_admin));
}

// ── Pause / unpause ─────────────────────────────────────────────

#[test]
fn deploys_revert_while_paused_for_all_kinds() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, admin, _fee_receiver, creator) = setup_launchpad(&env);

    client.pause();
    // Events are only retained for the last invocation, so check before any view call.
    assert!(event_present(
        &env,
        &client.address,
        (symbol_short!("paused"),).into_val(&env),
        (admin.clone(), true).into_val(&env),
    ));
    assert!(client.paused());

    let currency = Address::generate(&env);
    let royalty_receiver = Address::generate(&env);
    let creator_pubkey = BytesN::from_array(&env, &[0x06u8; 32]);

    assert_eq!(
        client.try_deploy_normal_721(
            &creator,
            &currency,
            &String::from_str(&env, "Paused 721"),
            &String::from_str(&env, "P721"),
            &100u64,
            &0u32,
            &royalty_receiver,
            &0u32,
            &BytesN::from_array(&env, &[0xB1u8; 32]),
        ),
        Err(Ok(Error::ContractPaused))
    );
    assert_eq!(
        client.try_deploy_normal_1155(
            &creator,
            &currency,
            &String::from_str(&env, "Paused 1155"),
            &0u32,
            &royalty_receiver,
            &0u32,
            &BytesN::from_array(&env, &[0xB2u8; 32]),
        ),
        Err(Ok(Error::ContractPaused))
    );
    assert_eq!(
        client.try_deploy_lazy_721(
            &creator,
            &currency,
            &creator_pubkey,
            &String::from_str(&env, "Paused L721"),
            &String::from_str(&env, "PL72"),
            &100u64,
            &0u32,
            &royalty_receiver,
            &0u32,
            &BytesN::from_array(&env, &[0xB3u8; 32]),
            &String::from_str(&env, TEST_PASSPHRASE),
        ),
        Err(Ok(Error::ContractPaused))
    );
    assert_eq!(
        client.try_deploy_lazy_1155(
            &creator,
            &currency,
            &creator_pubkey,
            &String::from_str(&env, "Paused L1155"),
            &0u32,
            &royalty_receiver,
            &0u32,
            &BytesN::from_array(&env, &[0xB4u8; 32]),
            &String::from_str(&env, TEST_PASSPHRASE),
        ),
        Err(Ok(Error::ContractPaused))
    );

    // After unpause, deployment works again.
    client.unpause();
    assert!(event_present(
        &env,
        &client.address,
        (symbol_short!("paused"),).into_val(&env),
        (admin.clone(), false).into_val(&env),
    ));
    assert!(!client.paused());

    client.deploy_normal_721(
        &creator,
        &currency,
        &String::from_str(&env, "Unpaused 721"),
        &String::from_str(&env, "U721"),
        &100u64,
        &0u32,
        &royalty_receiver,
        &0u32,
        &BytesN::from_array(&env, &[0xB5u8; 32]),
    );
    assert_eq!(client.collection_count(), 1u64);
}

// ── Fee config + flat deploy fee ────────────────────────────────

#[test]
fn set_fee_config_updates_config_and_emits_event() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, _creator) = setup_launchpad(&env);

    let new_receiver = Address::generate(&env);
    client.set_fee_config(&new_receiver, &1_000i128);

    assert!(event_present(
        &env,
        &client.address,
        (symbol_short!("cfg_fee"),).into_val(&env),
        (new_receiver.clone(), 1_000i128).into_val(&env),
    ));

    let (receiver, deploy_fee) = client.fee_config();
    assert_eq!(receiver, new_receiver);
    assert_eq!(deploy_fee, 1_000i128);
}

#[test]
fn set_fee_config_rejects_negative_fee() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, fee_receiver, _creator) = setup_launchpad(&env);

    assert_eq!(
        client.try_set_fee_config(&Address::generate(&env), &-100i128),
        Err(Ok(Error::InvalidDeployFee))
    );
    // Config unchanged.
    let (receiver, deploy_fee) = client.fee_config();
    assert_eq!(receiver, fee_receiver);
    assert_eq!(deploy_fee, 0i128);
}

/// With deploy_fee = 0, no token transfer is attempted (the currency here is
/// not even a token contract) and no fee event is emitted.
#[test]
fn zero_deploy_fee_charges_nothing_for_all_kinds() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, creator) = setup_launchpad(&env);

    let currency = Address::generate(&env); // not a token contract on purpose
    let royalty_receiver = Address::generate(&env);
    let creator_pubkey = BytesN::from_array(&env, &[0x07u8; 32]);

    client.deploy_normal_721(
        &creator,
        &currency,
        &String::from_str(&env, "Free 721"),
        &String::from_str(&env, "FR7"),
        &100u64,
        &0u32,
        &royalty_receiver,
        &0u32,
        &BytesN::from_array(&env, &[0xC1u8; 32]),
    );
    assert!(!event_with_tag_present(
        &env,
        &client.address,
        symbol_short!("fee_coll")
    ));

    client.deploy_normal_1155(
        &creator,
        &currency,
        &String::from_str(&env, "Free 1155"),
        &0u32,
        &royalty_receiver,
        &0u32,
        &BytesN::from_array(&env, &[0xC2u8; 32]),
    );
    assert!(!event_with_tag_present(
        &env,
        &client.address,
        symbol_short!("fee_coll")
    ));

    client.deploy_lazy_721(
        &creator,
        &currency,
        &creator_pubkey,
        &String::from_str(&env, "Free L721"),
        &String::from_str(&env, "FRL7"),
        &100u64,
        &0u32,
        &royalty_receiver,
        &0u32,
        &BytesN::from_array(&env, &[0xC3u8; 32]),
        &String::from_str(&env, TEST_PASSPHRASE),
    );
    assert!(!event_with_tag_present(
        &env,
        &client.address,
        symbol_short!("fee_coll")
    ));

    client.deploy_lazy_1155(
        &creator,
        &currency,
        &creator_pubkey,
        &String::from_str(&env, "Free L1155"),
        &0u32,
        &royalty_receiver,
        &0u32,
        &BytesN::from_array(&env, &[0xC4u8; 32]),
        &String::from_str(&env, TEST_PASSPHRASE),
    );
    assert!(!event_with_tag_present(
        &env,
        &client.address,
        symbol_short!("fee_coll")
    ));

    assert_eq!(client.collection_count(), 4u64);
}

/// With deploy_fee > 0, every deploy kind transfers exactly the flat fee to
/// the configured treasury and emits `fee_coll` with the correct receiver.
#[test]
fn flat_deploy_fee_charged_for_all_kinds() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, creator) = setup_launchpad(&env);

    const FEE: i128 = 250;
    let treasury = Address::generate(&env);
    client.set_fee_config(&treasury, &FEE);

    let token = setup_token(&env, &creator, 10_000);
    let token_client = TokenClient::new(&env, &token);

    let royalty_receiver = Address::generate(&env);
    let creator_pubkey = BytesN::from_array(&env, &[0x08u8; 32]);
    let fee_topics: soroban_sdk::Vec<Val> =
        (symbol_short!("fee_coll"), creator.clone(), treasury.clone()).into_val(&env);
    let fee_data: Val = (FEE, token.clone()).into_val(&env);

    client.deploy_normal_721(
        &creator,
        &token,
        &String::from_str(&env, "Paid 721"),
        &String::from_str(&env, "PD7"),
        &100u64,
        &0u32,
        &royalty_receiver,
        &0u32,
        &BytesN::from_array(&env, &[0xD1u8; 32]),
    );
    assert!(event_present(
        &env,
        &client.address,
        fee_topics.clone(),
        fee_data
    ));
    assert_eq!(token_client.balance(&treasury), FEE);

    client.deploy_normal_1155(
        &creator,
        &token,
        &String::from_str(&env, "Paid 1155"),
        &0u32,
        &royalty_receiver,
        &0u32,
        &BytesN::from_array(&env, &[0xD2u8; 32]),
    );
    assert!(event_present(
        &env,
        &client.address,
        fee_topics.clone(),
        fee_data
    ));
    assert_eq!(token_client.balance(&treasury), 2 * FEE);

    client.deploy_lazy_721(
        &creator,
        &token,
        &creator_pubkey,
        &String::from_str(&env, "Paid L721"),
        &String::from_str(&env, "PDL7"),
        &100u64,
        &0u32,
        &royalty_receiver,
        &0u32,
        &BytesN::from_array(&env, &[0xD3u8; 32]),
        &String::from_str(&env, TEST_PASSPHRASE),
    );
    assert!(event_present(
        &env,
        &client.address,
        fee_topics.clone(),
        fee_data
    ));
    assert_eq!(token_client.balance(&treasury), 3 * FEE);

    client.deploy_lazy_1155(
        &creator,
        &token,
        &creator_pubkey,
        &String::from_str(&env, "Paid L1155"),
        &0u32,
        &royalty_receiver,
        &0u32,
        &BytesN::from_array(&env, &[0xD4u8; 32]),
        &String::from_str(&env, TEST_PASSPHRASE),
    );
    assert!(event_present(&env, &client.address, fee_topics, fee_data));
    assert_eq!(token_client.balance(&treasury), 4 * FEE);

    assert_eq!(token_client.balance(&creator), 10_000 - 4 * FEE);
}

/// While the wasm hash is unset, no fee is charged even when configured.
#[test]
fn no_fee_charged_when_wasm_hash_unset() {
    let env = Env::default();
    env.mock_all_auths();

    let launchpad_id = env.register(Launchpad, ());
    let client = LaunchpadClient::new(&env, &launchpad_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let creator = Address::generate(&env);
    client.initialize(&admin, &treasury, &500i128);

    let token = setup_token(&env, &creator, 1_000);
    let royalty_receiver = Address::generate(&env);

    let result = client.try_deploy_normal_721(
        &creator,
        &token,
        &String::from_str(&env, "No Wasm Fee"),
        &String::from_str(&env, "NWF"),
        &100u64,
        &0u32,
        &royalty_receiver,
        &0u32,
        &BytesN::from_array(&env, &[0xD5u8; 32]),
    );
    assert_eq!(result, Err(Ok(Error::WasmHashNotSet)));
    assert_eq!(TokenClient::new(&env, &token).balance(&treasury), 0);
}

// ── WASM hash versioning ────────────────────────────────────────

#[test]
fn wasm_hashes_are_versioned_and_emit_events() {
    let env = Env::default();
    env.mock_all_auths();

    let launchpad_id = env.register(Launchpad, ());
    let client = LaunchpadClient::new(&env, &launchpad_id);

    let admin = Address::generate(&env);
    let fee_receiver = Address::generate(&env);
    client.initialize(&admin, &fee_receiver, &0i128);

    assert_eq!(client.wasm_version(), 0u32);
    assert_eq!(client.wasm_hashes(), None);

    let h1 = BytesN::from_array(&env, &[1u8; 32]);
    let h2 = BytesN::from_array(&env, &[2u8; 32]);
    let h3 = BytesN::from_array(&env, &[3u8; 32]);
    let h4 = BytesN::from_array(&env, &[4u8; 32]);

    let v = client.set_wasm_hashes(&h1, &h2, &h3, &h4);
    assert_eq!(v, 1u32);
    assert!(event_present(
        &env,
        &client.address,
        (symbol_short!("wasm_set"), 1u32).into_val(&env),
        (h1.clone(), h2.clone(), h3.clone(), h4.clone()).into_val(&env),
    ));

    let hashes = client.wasm_hashes().unwrap();
    assert_eq!(hashes.normal_721, h1);
    assert_eq!(hashes.normal_1155, h2);
    assert_eq!(hashes.lazy_721, h3);
    assert_eq!(hashes.lazy_1155, h4);
    assert_eq!(hashes.version, 1u32);

    // A second upload bumps the version so indexers can track the upgrade.
    let h5 = BytesN::from_array(&env, &[5u8; 32]);
    let v = client.set_wasm_hashes(&h5, &h2, &h3, &h4);
    assert_eq!(v, 2u32);
    assert!(event_present(
        &env,
        &client.address,
        (symbol_short!("wasm_set"), 2u32).into_val(&env),
        (h5.clone(), h2.clone(), h3.clone(), h4.clone()).into_val(&env),
    ));

    let hashes = client.wasm_hashes().unwrap();
    assert_eq!(hashes.normal_721, h5);
    assert_eq!(hashes.version, 2u32);
    assert_eq!(client.wasm_version(), 2u32);
}

// ── View function tests ─────────────────────────────────────────

#[test]
fn view_functions_return_correct_values() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, admin, fee_receiver, _creator) = setup_launchpad(&env);

    assert_eq!(client.admin(), admin);
    assert_eq!(client.pending_admin(), None);
    assert!(!client.paused());

    let (receiver, deploy_fee) = client.fee_config();
    assert_eq!(receiver, fee_receiver);
    assert_eq!(deploy_fee, 0i128);

    // setup_launchpad calls set_wasm_hashes once.
    assert_eq!(client.wasm_version(), 1u32);
}

#[test]
fn update_collection_wasm_and_upgrade_collection_emit_events() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, creator) = setup_launchpad(&env);
    // upgrade() inside lazy_721 calls creator.require_auth() from a sub-contract
    // invocation (launchpad → lazy_721). mock_all_auths() only covers root-level
    // auth; we need the non-root variant here.
    env.mock_all_auths_allowing_non_root_auth();

    let currency = Address::generate(&env);
    let royalty_receiver = Address::generate(&env);
    let creator_pubkey = BytesN::from_array(&env, &[0xBBu8; 32]);
    // lazy_721 has an `upgrade` function; normal_721 does not.
    let deployed = client.deploy_lazy_721(
        &creator,
        &currency,
        &creator_pubkey,
        &String::from_str(&env, "Upgradeable L721"),
        &String::from_str(&env, "UPL7"),
        &100u64,
        &500u32,
        &royalty_receiver,
        &0u32,
        &BytesN::from_array(&env, &[0xAAu8; 32]),
        &String::from_str(&env, TEST_PASSPHRASE),
    );

    // Upload a fresh copy of the same WASM as the "new" version.
    let new_wasm_bytes = wasm_bytes("lazy_mint_erc721");
    let wasm_v2 = env.deployer().upload_contract_wasm(new_wasm_bytes.as_slice());

    client.update_collection_wasm(&CollectionKind::LazyMint721, &wasm_v2);
    assert!(event_with_tag_present(&env, &client.address, symbol_short!("wasm_upd")));

    client.upgrade_collection(&deployed);
    assert!(event_with_tag_present(&env, &client.address, symbol_short!("col_upgr")));
}

// ── Collections view tests ──────────────────────────────────────

#[test]
fn collections_by_creator_returns_correct_collections() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, creator) = setup_launchpad(&env);

    let other = Address::generate(&env);
    let salt = BytesN::from_array(&env, &[0x55u8; 32]);
    let currency = Address::generate(&env);
    let royalty_receiver = Address::generate(&env);

    client.deploy_normal_721(
        &creator,
        &currency,
        &String::from_str(&env, "Creator Coll"),
        &String::from_str(&env, "CRC"),
        &100u64,
        &500u32,
        &royalty_receiver,
        &0u32,
        &salt,
    );

    let creator_colls = client.collections_by_creator(&creator);
    assert_eq!(creator_colls.len(), 1);
    assert!(matches!(
        creator_colls.get(0).unwrap().kind,
        CollectionKind::Normal721
    ));

    let other_colls = client.collections_by_creator(&other);
    assert_eq!(other_colls.len(), 0);
}

// ── Issue #201: Invalid ED25519 signature and expired voucher tests ───────────
//
// Deploy a lazy_721 via the launchpad, then verify the deployed collection
// rejects invalid ED25519 signatures and expired vouchers.
//
// We mirror the MintVoucher / Error types from lazy_mint_erc721 using the same
// #[contracttype] / #[contracterror] macros so the XDR encoding matches.

use soroban_sdk::{contractclient, contracterror, contracttype};

#[contracttype]
#[derive(Clone)]
pub struct MintVoucher {
    pub token_id: u64,
    pub nonce: u64,
    pub price: i128,
    pub currency: Address,
    pub uri: String,
    pub uri_hash: BytesN<32>,
    pub valid_until: u64,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum LazyError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    NotOwner = 3,
    NotApproved = 4,
    TokenNotFound = 5,
    MaxSupplyReached = 6,
    VoucherExpired = 7,
    VoucherAlreadyRedeemed = 8,
    NotCreator = 9,
    InvalidSignature = 10,
    NotAllowlisted = 11,
    InvalidMerkleProof = 12,
}

#[contractclient(name = "Lazy721Client")]
pub trait ILazy721 {
    fn redeem(
        env: Env,
        buyer: Address,
        voucher: MintVoucher,
        signature: BytesN<64>,
        merkle_proof: soroban_sdk::Vec<BytesN<32>>,
    ) -> Result<u64, LazyError>;

    /// Switch the sale to public phase so redeem skips the allowlist check.
    fn set_public_phase(env: Env) -> Result<(), LazyError>;
}

/// Both lazy-mint contracts expose `platform_fee_info() -> (Address, u32)`.
#[contractclient(name = "FeeInfoClient")]
pub trait IFeeInfo {
    fn platform_fee_info(env: Env) -> (Address, u32);
}

/// After deploying a lazy_721 via the launchpad, redeeming with an invalid
/// ED25519 signature must be rejected by the deployed collection contract.
#[test]
fn deployed_lazy_721_rejects_invalid_ed25519_signature() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, creator) = setup_launchpad(&env);

    let creator_pubkey = BytesN::from_array(&env, &[1u8; 32]);
    let royalty_receiver = Address::generate(&env);
    let currency = Address::generate(&env);
    let salt = BytesN::from_array(&env, &[0xA1u8; 32]);

    let collection_addr = client.deploy_lazy_721(
        &creator,
        &currency,
        &creator_pubkey,
        &String::from_str(&env, "Sig Test 721"),
        &String::from_str(&env, "ST7"),
        &1_000u64,
        &0u32,
        &royalty_receiver,
        &0u32,
        &salt,
        &String::from_str(&env, TEST_PASSPHRASE),
    );

    let lazy_client = Lazy721Client::new(&env, &collection_addr);
    lazy_client.set_public_phase();
    let buyer = Address::generate(&env);
    let voucher = MintVoucher {
        token_id: 1,
        nonce: 1,
        price: 0,
        currency: Address::generate(&env),
        uri: String::from_str(&env, "ipfs://test"),
        uri_hash: BytesN::from_array(&env, &[0u8; 32]),
        valid_until: 0,
    };

    // All-zeros is not a valid ed25519 signature — host will abort
    let bad_sig = BytesN::from_array(&env, &[0u8; 64]);
    let result = lazy_client.try_redeem(&buyer, &voucher, &bad_sig, &soroban_sdk::vec![&env]);
    assert!(result.is_err(), "invalid signature must be rejected");
}

/// After deploying a lazy_721 via the launchpad, redeeming an expired voucher
/// (valid_until < current ledger sequence) must return VoucherExpired.
#[test]
fn deployed_lazy_721_rejects_expired_voucher() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, creator) = setup_launchpad(&env);

    let creator_pubkey = BytesN::from_array(&env, &[2u8; 32]);
    let royalty_receiver = Address::generate(&env);
    let currency = Address::generate(&env);
    let salt = BytesN::from_array(&env, &[0xA2u8; 32]);

    let collection_addr = client.deploy_lazy_721(
        &creator,
        &currency,
        &creator_pubkey,
        &String::from_str(&env, "Expiry Test 721"),
        &String::from_str(&env, "ET7"),
        &1_000u64,
        &0u32,
        &royalty_receiver,
        &0u32,
        &salt,
        &String::from_str(&env, TEST_PASSPHRASE),
    );

    let lazy_client = Lazy721Client::new(&env, &collection_addr);
    lazy_client.set_public_phase();

    // Advance ledger past the voucher's valid_until
    env.ledger().with_mut(|li| li.sequence_number = 200);

    let buyer = Address::generate(&env);
    let voucher = MintVoucher {
        token_id: 1,
        nonce: 1,
        price: 0,
        currency: Address::generate(&env),
        uri: String::from_str(&env, "ipfs://expired"),
        uri_hash: BytesN::from_array(&env, &[0u8; 32]),
        valid_until: 50, // expired: 50 < 200
    };

    let sig = BytesN::from_array(&env, &[0u8; 64]);
    let result = lazy_client.try_redeem(&buyer, &voucher, &sig, &soroban_sdk::vec![&env]);
    assert_eq!(
        result,
        Err(Ok(LazyError::VoucherExpired)),
        "expired voucher must return VoucherExpired"
    );
}

// ── Lazy deploy: initialize args propagation (regression for the paths that
//    previously did not compile) ─────────────────────────────────────────────

/// deploy_lazy_721 forwards the factory fee receiver and the caller-chosen
/// platform_fee_bps to the child contract's initialize.
#[test]
fn lazy_721_receives_platform_fee_receiver_and_bps() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, fee_receiver, creator) = setup_launchpad(&env);

    let creator_pubkey = BytesN::from_array(&env, &[0x0Au8; 32]);
    let royalty_receiver = Address::generate(&env);
    let currency = Address::generate(&env);

    let addr = client.deploy_lazy_721(
        &creator,
        &currency,
        &creator_pubkey,
        &String::from_str(&env, "Prop L721"),
        &String::from_str(&env, "PRL7"),
        &100u64,
        &0u32,
        &royalty_receiver,
        &750u32,
        &BytesN::from_array(&env, &[0xA3u8; 32]),
        &String::from_str(&env, TEST_PASSPHRASE),
    );

    let (child_receiver, child_bps) = FeeInfoClient::new(&env, &addr).platform_fee_info();
    assert_eq!(child_receiver, fee_receiver);
    assert_eq!(child_bps, 750u32);
}

/// deploy_lazy_1155 forwards the factory fee receiver and the caller-chosen
/// platform_fee_bps to the child contract's initialize.
#[test]
fn lazy_1155_receives_platform_fee_receiver_and_bps() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, fee_receiver, creator) = setup_launchpad(&env);

    let creator_pubkey = BytesN::from_array(&env, &[0x0Bu8; 32]);
    let royalty_receiver = Address::generate(&env);
    let currency = Address::generate(&env);

    let addr = client.deploy_lazy_1155(
        &creator,
        &currency,
        &creator_pubkey,
        &String::from_str(&env, "Prop L1155"),
        &0u32,
        &royalty_receiver,
        &900u32,
        &BytesN::from_array(&env, &[0xA4u8; 32]),
        &String::from_str(&env, TEST_PASSPHRASE),
    );

    let (child_receiver, child_bps) = FeeInfoClient::new(&env, &addr).platform_fee_info();
    assert_eq!(child_receiver, fee_receiver);
    assert_eq!(child_bps, 900u32);
}

/// The treasury configured via set_fee_config (not the initialize-time value)
/// is what the lazy child receives as platform_fee_receiver.
#[test]
fn lazy_deploy_uses_current_fee_config_receiver() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, creator) = setup_launchpad(&env);

    let new_treasury = Address::generate(&env);
    client.set_fee_config(&new_treasury, &0i128);

    let creator_pubkey = BytesN::from_array(&env, &[0x0Cu8; 32]);
    let royalty_receiver = Address::generate(&env);
    let currency = Address::generate(&env);

    let addr = client.deploy_lazy_721(
        &creator,
        &currency,
        &creator_pubkey,
        &String::from_str(&env, "Treasury L721"),
        &String::from_str(&env, "TRL7"),
        &100u64,
        &0u32,
        &royalty_receiver,
        &100u32,
        &BytesN::from_array(&env, &[0xA5u8; 32]),
        &String::from_str(&env, TEST_PASSPHRASE),
    );

    let (child_receiver, _) = FeeInfoClient::new(&env, &addr).platform_fee_info();
    assert_eq!(child_receiver, new_treasury);
}

// ─── Issue #37 — Registry metadata tests ─────────────────────────────────────

/// deploy_normal_721 stores name, symbol, and ledger in the collection record.
#[test]
fn registry_stores_full_metadata_normal_721() {
    let env = Env::default();
    env.ledger().with_mut(|li| {
        li.sequence_number = 42;
    });
    let (client, _admin, _fee_receiver, creator) = setup_launchpad(&env);

    let salt = BytesN::from_array(&env, &[0xF1u8; 32]);
    let currency = Address::generate(&env);
    let royalty_receiver = Address::generate(&env);

    let addr = client.deploy_normal_721(
        &creator,
        &currency,
        &String::from_str(&env, "My Collection"),
        &String::from_str(&env, "MYC"),
        &500u64,
        &0u32,
        &royalty_receiver,
        &0u32, // platform_fee_bps
        &salt,
    );

    let record = client.get_collection(&addr).unwrap();
    assert_eq!(record.name, String::from_str(&env, "My Collection"));
    assert_eq!(record.symbol, String::from_str(&env, "MYC"));
    assert_eq!(record.ledger, 42u32);
    assert_eq!(record.platform_fee_bps, 0u32);
    assert_eq!(record.creator, creator);
}

/// get_collection returns the same record as all_collections for the same address.
#[test]
fn get_collection_matches_all_collections() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, creator) = setup_launchpad(&env);

    let salt = BytesN::from_array(&env, &[0xF2u8; 32]);
    let currency = Address::generate(&env);
    let royalty_receiver = Address::generate(&env);

    let addr = client.deploy_normal_721(
        &creator,
        &currency,
        &String::from_str(&env, "Alpha"),
        &String::from_str(&env, "ALP"),
        &100u64,
        &0u32,
        &royalty_receiver,
        &0u32,
        &salt,
    );

    let by_addr = client.get_collection(&addr).unwrap();
    let all = client.all_collections();
    let from_all = all.get(0).unwrap();
    assert_eq!(by_addr.address, from_all.address);
    assert_eq!(by_addr.name, from_all.name);
}

/// get_collections with start=0, limit=2 returns first two records in order.
#[test]
fn get_collections_paginated_returns_correct_slice() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, creator) = setup_launchpad(&env);

    let currency = Address::generate(&env);
    let royalty_receiver = Address::generate(&env);

    let salt_a = BytesN::from_array(&env, &[0xF3u8; 32]);
    let salt_b = BytesN::from_array(&env, &[0xF4u8; 32]);
    let salt_c = BytesN::from_array(&env, &[0xF5u8; 32]);

    client.deploy_normal_721(
        &creator,
        &currency,
        &String::from_str(&env, "Alpha"),
        &String::from_str(&env, "ALP"),
        &100u64,
        &0u32,
        &royalty_receiver,
        &0u32,
        &salt_a,
    );
    client.deploy_normal_721(
        &creator,
        &currency,
        &String::from_str(&env, "Beta"),
        &String::from_str(&env, "BET"),
        &100u64,
        &0u32,
        &royalty_receiver,
        &0u32,
        &salt_b,
    );
    client.deploy_normal_721(
        &creator,
        &currency,
        &String::from_str(&env, "Gamma"),
        &String::from_str(&env, "GAM"),
        &100u64,
        &0u32,
        &royalty_receiver,
        &0u32,
        &salt_c,
    );

    assert_eq!(client.collection_count(), 3u64);

    let page0 = client.get_collections(&0u64, &2u32);
    assert_eq!(page0.len(), 2);
    assert_eq!(page0.get(0).unwrap().name, String::from_str(&env, "Alpha"));
    assert_eq!(page0.get(1).unwrap().name, String::from_str(&env, "Beta"));

    let page1 = client.get_collections(&2u64, &2u32);
    assert_eq!(page1.len(), 1);
    assert_eq!(page1.get(0).unwrap().name, String::from_str(&env, "Gamma"));
}

/// get_collections beyond range returns empty vec.
#[test]
fn get_collections_out_of_range_returns_empty() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, creator) = setup_launchpad(&env);

    let currency = Address::generate(&env);
    let royalty_receiver = Address::generate(&env);
    let salt = BytesN::from_array(&env, &[0xF6u8; 32]);

    client.deploy_normal_721(
        &creator,
        &currency,
        &String::from_str(&env, "Only"),
        &String::from_str(&env, "ONL"),
        &100u64,
        &0u32,
        &royalty_receiver,
        &0u32,
        &salt,
    );

    let empty = client.get_collections(&10u64, &5u32);
    assert_eq!(empty.len(), 0);
}

// ─── Issue #38 — Per-collection platform fee tests ────────────────────────────

/// Deploying with platform_fee_bps > MAX_FEE_BPS (2000) is rejected.
#[test]
fn invalid_fee_bps_rejected_at_deploy() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, creator) = setup_launchpad(&env);

    let salt = BytesN::from_array(&env, &[0xE1u8; 32]);
    let currency = Address::generate(&env);
    let royalty_receiver = Address::generate(&env);

    let result = client.try_deploy_normal_721(
        &creator,
        &currency,
        &String::from_str(&env, "Fee Test"),
        &String::from_str(&env, "FEE"),
        &100u64,
        &0u32,
        &royalty_receiver,
        &2001u32, // exceeds MAX_FEE_BPS
        &salt,
    );
    assert_eq!(result, Err(Ok(Error::InvalidFeeBps)));
}

/// Deploying with platform_fee_bps = MAX_FEE_BPS (2000) succeeds.
#[test]
fn valid_fee_bps_at_max_boundary_succeeds() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, creator) = setup_launchpad(&env);

    let salt = BytesN::from_array(&env, &[0xE2u8; 32]);
    let currency = Address::generate(&env);
    let royalty_receiver = Address::generate(&env);

    let addr = client.deploy_normal_721(
        &creator,
        &currency,
        &String::from_str(&env, "Max Fee"),
        &String::from_str(&env, "MXF"),
        &100u64,
        &0u32,
        &royalty_receiver,
        &2000u32, // exactly MAX_FEE_BPS
        &salt,
    );

    let record = client.get_collection(&addr).unwrap();
    assert_eq!(record.platform_fee_bps, 2000u32);
}

/// Configured fee is persisted in the collection record for all 4 deploy types.
#[test]
fn fee_stored_in_collection_record_for_all_types() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, creator) = setup_launchpad(&env);

    let currency = Address::generate(&env);
    let royalty_receiver = Address::generate(&env);
    let creator_pubkey = BytesN::from_array(&env, &[0xAAu8; 32]);

    let addr_721 = client.deploy_normal_721(
        &creator,
        &currency,
        &String::from_str(&env, "N721"),
        &String::from_str(&env, "N721"),
        &100u64,
        &0u32,
        &royalty_receiver,
        &500u32,
        &BytesN::from_array(&env, &[0xE3u8; 32]),
    );
    assert_eq!(
        client.get_collection(&addr_721).unwrap().platform_fee_bps,
        500u32
    );

    let addr_1155 = client.deploy_normal_1155(
        &creator,
        &currency,
        &String::from_str(&env, "N1155"),
        &0u32,
        &royalty_receiver,
        &750u32,
        &BytesN::from_array(&env, &[0xE4u8; 32]),
    );
    assert_eq!(
        client.get_collection(&addr_1155).unwrap().platform_fee_bps,
        750u32
    );

    let addr_l721 = client.deploy_lazy_721(
        &creator,
        &currency,
        &creator_pubkey,
        &String::from_str(&env, "L721"),
        &String::from_str(&env, "L721"),
        &100u64,
        &0u32,
        &royalty_receiver,
        &100u32,
        &BytesN::from_array(&env, &[0xE5u8; 32]),
        &String::from_str(&env, TEST_PASSPHRASE),
    );
    assert_eq!(
        client.get_collection(&addr_l721).unwrap().platform_fee_bps,
        100u32
    );

    let addr_l1155 = client.deploy_lazy_1155(
        &creator,
        &currency,
        &creator_pubkey,
        &String::from_str(&env, "L1155"),
        &0u32,
        &royalty_receiver,
        &200u32,
        &BytesN::from_array(&env, &[0xE6u8; 32]),
        &String::from_str(&env, TEST_PASSPHRASE),
    );
    assert_eq!(
        client.get_collection(&addr_l1155).unwrap().platform_fee_bps,
        200u32
    );
}

/// Invalid fee rejected for all 4 deploy function variants.
#[test]
fn invalid_fee_rejected_for_all_deploy_variants() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, creator) = setup_launchpad(&env);

    let currency = Address::generate(&env);
    let royalty_receiver = Address::generate(&env);
    let creator_pubkey = BytesN::from_array(&env, &[0xBBu8; 32]);

    assert_eq!(
        client.try_deploy_normal_1155(
            &creator,
            &currency,
            &String::from_str(&env, "Bad Fee 1155"),
            &0u32,
            &royalty_receiver,
            &9999u32,
            &BytesN::from_array(&env, &[0xF7u8; 32]),
        ),
        Err(Ok(Error::InvalidFeeBps))
    );

    assert_eq!(
        client.try_deploy_lazy_721(
            &creator,
            &currency,
            &creator_pubkey,
            &String::from_str(&env, "Bad Fee L721"),
            &String::from_str(&env, "BFL"),
            &100u64,
            &0u32,
            &royalty_receiver,
            &5000u32,
            &BytesN::from_array(&env, &[0xF8u8; 32]),
            &String::from_str(&env, TEST_PASSPHRASE),
        ),
        Err(Ok(Error::InvalidFeeBps))
    );

    assert_eq!(
        client.try_deploy_lazy_1155(
            &creator,
            &currency,
            &creator_pubkey,
            &String::from_str(&env, "Bad Fee L1155"),
            &0u32,
            &royalty_receiver,
            &3000u32,
            &BytesN::from_array(&env, &[0xF9u8; 32]),
            &String::from_str(&env, TEST_PASSPHRASE),
        ),
        Err(Ok(Error::InvalidFeeBps))
    );
}

// ─── Preflight validation (#277) ───────────────────────────────────────────
//
// These tests prove that `preflight_deploy_*` and `deploy_*` agree: a clean
// preflight (no errors) means the deploy succeeds at the *same* predicted
// address, and every error the preflight reports is one the deploy call
// would also raise.

#[test]
fn preflight_normal_721_predicts_the_deployed_address() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, creator) = setup_launchpad(&env);

    let salt = BytesN::from_array(&env, &[40u8; 32]);
    let royalty_receiver = Address::generate(&env);
    let currency = setup_token(&env, &creator, 1_000_000);

    let preflight = client.preflight_deploy_normal_721(
        &creator,
        &currency,
        &String::from_str(&env, "Preflight 721"),
        &String::from_str(&env, "PF721"),
        &1_000u64,
        &500u32,
        &0u32,
        &salt,
    );
    assert!(preflight.errors.is_empty());

    let deployed = client.deploy_normal_721(
        &creator,
        &currency,
        &String::from_str(&env, "Preflight 721"),
        &String::from_str(&env, "PF721"),
        &1_000u64,
        &500u32,
        &royalty_receiver,
        &0u32,
        &salt,
    );

    assert_eq!(preflight.predicted_address, deployed);

    // Re-running preflight against the now-consumed salt must surface the
    // duplicate-salt error — and the real deploy call must reject it too.
    let preflight_after = client.preflight_deploy_normal_721(
        &creator,
        &currency,
        &String::from_str(&env, "Preflight 721 Dup"),
        &String::from_str(&env, "PF72D"),
        &1_000u64,
        &500u32,
        &0u32,
        &salt,
    );
    assert_eq!(preflight_after.predicted_address, deployed);
    assert!(preflight_after.errors.contains(&(Error::DuplicateSalt as u32)));

    assert_eq!(
        client.try_deploy_normal_721(
            &creator,
            &currency,
            &String::from_str(&env, "Preflight 721 Dup"),
            &String::from_str(&env, "PF72D"),
            &1_000u64,
            &500u32,
            &royalty_receiver,
            &0u32,
            &salt,
        ),
        Err(Ok(Error::DuplicateSalt))
    );
}

#[test]
fn preflight_normal_721_reports_every_error_deploy_would_raise() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, creator) = setup_launchpad(&env);

    client.pause();

    let salt = BytesN::from_array(&env, &[41u8; 32]);
    let currency = setup_token(&env, &creator, 1_000_000);

    // Stack multiple violations at once: paused, empty name, empty symbol,
    // zero max_supply, royalty over 100%, and platform fee over the cap.
    let preflight = client.preflight_deploy_normal_721(
        &creator,
        &currency,
        &String::from_str(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &10_001u32,
        &2001u32,
        &salt,
    );

    assert!(preflight.errors.contains(&(Error::ContractPaused as u32)));
    assert!(preflight.errors.contains(&(Error::EmptyName as u32)));
    assert!(preflight.errors.contains(&(Error::EmptySymbol as u32)));
    assert!(preflight.errors.contains(&(Error::InvalidMaxSupply as u32)));
    assert!(preflight.errors.contains(&(Error::InvalidRoyaltyBps as u32)));
    assert!(preflight.errors.contains(&(Error::InvalidFeeBps as u32)));

    // The real deploy call rejects on the first violation it checks
    // (contract-paused) — proving preflight is a superset, never blind to a
    // failure the mutating call would also hit.
    let royalty_receiver = Address::generate(&env);
    assert_eq!(
        client.try_deploy_normal_721(
            &creator,
            &currency,
            &String::from_str(&env, ""),
            &String::from_str(&env, ""),
            &0u64,
            &10_001u32,
            &royalty_receiver,
            &2001u32,
            &salt,
        ),
        Err(Ok(Error::ContractPaused))
    );
}

#[test]
fn preflight_flags_insufficient_balance_for_the_flat_fee() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, fee_receiver, creator) = setup_launchpad(&env);

    // Configure a flat deploy fee the creator cannot cover.
    client.set_fee_config(&fee_receiver, &1_000i128);

    let salt = BytesN::from_array(&env, &[42u8; 32]);
    let currency = setup_token(&env, &creator, 10); // far below the 1_000 fee

    let preflight = client.preflight_deploy_normal_1155(
        &creator,
        &currency,
        &String::from_str(&env, "Poor Creator"),
        &500u32,
        &0u32,
        &salt,
    );

    assert!(preflight.errors.contains(&(Error::InsufficientFee as u32)));
    assert_eq!(preflight.required_fee, 1_000i128);

    // The real deploy call fails too — it attempts the token transfer, which
    // panics on insufficient balance (Soroban SAC traps rather than
    // returning a typed error, so we only assert preflight caught it ahead
    // of the mutating, fee-charging call).
    let deploy_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.deploy_normal_1155(
            &creator,
            &currency,
            &String::from_str(&env, "Poor Creator"),
            &500u32,
            &Address::generate(&env),
            &0u32,
            &salt,
        )
    }));
    assert!(deploy_result.is_err());
}

#[test]
fn preflight_lazy_1155_predicts_the_deployed_address_and_matches_deploy_errors() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.sequence_number = 1);
    let (client, _admin, _fee_receiver, creator) = setup_launchpad(&env);

    let salt = BytesN::from_array(&env, &[43u8; 32]);
    let creator_pubkey = BytesN::from_array(&env, &[9u8; 32]);
    let royalty_receiver = Address::generate(&env);
    let currency = setup_token(&env, &creator, 1_000_000);

    let preflight = client.preflight_deploy_lazy_1155(
        &creator,
        &currency,
        &String::from_str(&env, "Preflight Lazy 1155"),
        &500u32,
        &0u32,
        &salt,
    );
    assert!(preflight.errors.is_empty());

    let deployed = client.deploy_lazy_1155(
        &creator,
        &currency,
        &creator_pubkey,
        &String::from_str(&env, "Preflight Lazy 1155"),
        &500u32,
        &royalty_receiver,
        &0u32,
        &salt,
        &String::from_str(&env, TEST_PASSPHRASE),
    );

    assert_eq!(preflight.predicted_address, deployed);
}
