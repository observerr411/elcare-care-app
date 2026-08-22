use soroban_sdk::{contracterror, contracttype, Address, BytesN, String, Vec};

/// Semantic contract version — bump on every breaking storage change.
pub const CONTRACT_VERSION: &str = "1.0.0";

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    NotAdmin = 3,
    WasmHashNotSet = 4,
    InvalidFeeBps = 5,
    ContractPaused = 6,
    InvalidDeployFee = 7,
    NoPendingAdmin = 8,
    NotPendingAdmin = 9,
    /// The (creator, salt) pair has already been used for a deployment (#277).
    DuplicateSalt = 10,
    /// royalty_bps exceeds 10_000 (100%) (#277).
    InvalidRoyaltyBps = 11,
    /// Collection name is empty (#277).
    EmptyName = 12,
    /// Collection symbol is empty for a kind that requires one (#277).
    EmptySymbol = 13,
    /// max_supply is zero (#277).
    InvalidMaxSupply = 14,
    /// Creator's balance of `currency` is insufficient to cover the deploy fee (#277).
    InsufficientFee = 15,
    /// migrate() called for a version that has already been migrated.
    AlreadyMigrated = 16,
    /// upgrade_collection() called with an address not in the registry.
    CollectionNotFound = 17,
}

/// Which of the four collection types was deployed.
#[contracttype]
#[derive(Clone)]
pub enum CollectionKind {
    Normal721,
    Normal1155,
    LazyMint721,
    LazyMint1155,
}

/// A record stored for every deployed collection (issues #37 + #38).
#[contracttype]
#[derive(Clone)]
pub struct CollectionRecord {
    pub address: Address,
    pub kind: CollectionKind,
    pub creator: Address,
    pub name: String,
    pub symbol: String,
    pub ledger: u32,
    pub platform_fee_bps: u32,
}

/// The four collection WASM hashes plus a monotonically increasing version,
/// bumped on every `set_wasm_hashes` so indexers can track factory upgrades.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WasmHashes {
    pub normal_721: BytesN<32>,
    pub normal_1155: BytesN<32>,
    pub lazy_721: BytesN<32>,
    pub lazy_1155: BytesN<32>,
    pub version: u32,
}

/// Result of a read-only `preflight_deploy_*` call (#277). Lets creators and
/// operators validate the exact deployment inputs before submitting a
/// transaction: the predicted deterministic address, the flat fee that will
/// be charged, and the full set of validation failures (empty when the
/// matching `deploy_*` call is expected to succeed).
#[contracttype]
#[derive(Clone)]
pub struct PreflightResult {
    /// The address the collection would be deployed to — identical to the
    /// address returned by the matching `deploy_*` call given the same
    /// (creator, salt) pair.
    pub predicted_address: Address,
    /// The flat `deploy_fee` (token smallest unit) that will be charged in
    /// `currency`. Zero when no flat fee is configured.
    pub required_fee: i128,
    /// The per-collection platform fee (bps) that would be recorded in the
    /// registry, echoed back for convenience.
    pub platform_fee_bps: u32,
    /// The currency the required fee would be charged in.
    pub currency: Address,
    /// Every validation failure that the matching `deploy_*` call would
    /// raise given identical inputs, encoded as their `Error` discriminant
    /// `u32` values.  Empty means the deployment is expected to succeed.
    pub errors: Vec<u32>,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Initialized,
    Admin,
    PendingAdmin,
    Paused,
    /// Treasury address receiving the flat deployment fee; also forwarded to
    /// lazy-mint contracts as their `platform_fee_receiver`.
    FeeReceiver,
    /// Flat deployment fee in the deploy currency's smallest unit (i128).
    DeployFee,
    WasmNormal721,
    WasmNormal1155,
    WasmLazy721,
    WasmLazy1155,
    /// Active WASM hash for a specific collection kind.
    CollectionWasmHash(CollectionKind),
    /// Incremented on every `set_wasm_hashes`.
    WasmVersion,
    CollectionCount,
    ByCreator(Address),
    AllCollections,
    CollectionByIndex(u64),
    CreatorCollectionCount(Address),
    CreatorCollectionByIndex(Address, u64),
    /// Direct lookup by collection address (#37)
    CollectionByAddress(Address),
    /// Marks a (creator, raw_salt) pair — hashed into the secure salt — as
    /// already consumed by a successful deployment (#277).
    SaltUsed(BytesN<32>),
    /// Explicit holder of the `EmergencyPause` role (Issue #267). Absent
    /// until `set_emergency_pauser` is called; `pause`/`unpause` fall back to
    /// `Admin` while absent so existing single-admin deployments are
    /// unaffected until an operator opts into a separate emergency signer.
    EmergencyPauser,
    /// Persistent marker — set when a versioned migration completes.
    MigrationDone(String),
    /// Persistent resumable progress for a versioned migration.
    MigrationCursor(String),
    /// Instance-storage slot holding the version string last written by migrate().
    ContractVersion,
}
