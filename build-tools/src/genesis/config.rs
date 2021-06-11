use std::convert::TryFrom;

use chrono::{DateTime, Utc};
use hex::FromHex;
use serde::de::Error;
use serde::{Deserialize, Deserializer};

use beserial::Deserialize as BDeserialize;
use bls::{PublicKey as BlsPublicKey, SecretKey as BlsSecretKey};
use keys::Address;
use primitives::account::ValidatorId;
use primitives::coin::Coin;
use transaction::account::htlc_contract::{AnyHash, HashAlgorithm};

#[derive(Clone, Debug, Deserialize)]
pub struct GenesisConfig {
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_bls_secret_key_opt")]
    pub signing_key: Option<BlsSecretKey>,

    pub seed_message: Option<String>,

    pub timestamp: Option<DateTime<Utc>>,

    #[serde(default)]
    pub nim_1_head_block: Option<NimiqLegacyHeadBlock>,

    #[serde(default)]
    pub validators: Vec<GenesisValidator>,

    #[serde(default)]
    pub stakes: Vec<GenesisStake>,

    #[serde(default)]
    pub basic_accounts: Vec<GenesisBasicAccount>,

    #[serde(default)]
    pub vesting_accounts: Vec<GenesisVestingAccount>,

    #[serde(default)]
    pub htlc_accounts: Vec<GenesisHTLCAccount>,

    #[serde(default)]
    #[serde(deserialize_with = "deserialize_nimiq_address_opt")]
    pub staking_contract: Option<Address>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct NimiqLegacyHeadBlock {
    pub height: u64,

    pub timestamp: u64,

    pub custom_genesis_delay: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GenesisValidator {
    #[serde(deserialize_with = "deserialize_validator_id")]
    pub validator_id: ValidatorId,

    #[serde(deserialize_with = "deserialize_nimiq_address")]
    pub reward_address: Address,

    #[serde(deserialize_with = "deserialize_coin")]
    pub balance: Coin,

    #[serde(deserialize_with = "deserialize_bls_public_key")]
    pub validator_key: BlsPublicKey,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GenesisStake {
    #[serde(deserialize_with = "deserialize_nimiq_address")]
    pub staker_address: Address,

    #[serde(deserialize_with = "deserialize_coin")]
    pub balance: Coin,

    #[serde(deserialize_with = "deserialize_validator_id")]
    pub validator_id: ValidatorId,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GenesisBasicAccount {
    #[serde(deserialize_with = "deserialize_nimiq_address")]
    pub address: Address,

    #[serde(deserialize_with = "deserialize_coin")]
    pub balance: Coin,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GenesisVestingAccount {
    #[serde(deserialize_with = "deserialize_nimiq_address")]
    pub address: Address,

    #[serde(deserialize_with = "deserialize_coin")]
    pub balance: Coin,

    #[serde(deserialize_with = "deserialize_nimiq_address")]
    pub owner: Address,

    pub vesting_start : u64,

    pub vesting_start_ts : Option<u64>,

    pub vesting_step_blocks: u64,

    #[serde(deserialize_with = "deserialize_coin")]
    pub vesting_step_amount: Coin,

    #[serde(deserialize_with = "deserialize_coin")]
    pub vesting_total_amount: Coin,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GenesisHTLCAccount {
    #[serde(deserialize_with = "deserialize_nimiq_address")]
    pub address: Address,

    #[serde(deserialize_with = "deserialize_coin")]
    pub balance: Coin,

    #[serde(deserialize_with = "deserialize_nimiq_address")]
    pub sender: Address,

    #[serde(deserialize_with = "deserialize_nimiq_hash_algorithm")]
    pub hash_algorithm: HashAlgorithm,

    #[serde(deserialize_with = "deserialize_nimiq_hash")]
    pub hash_root: AnyHash,

    #[serde(deserialize_with = "deserialize_nimiq_address")]
    pub recipient: Address,

    pub hash_count: u8,

    pub timeout: u64,

    pub timeout_ts: Option<u64>,

    #[serde(deserialize_with = "deserialize_coin")]
    pub total_amount: Coin,
}

pub fn deserialize_nimiq_address<'de, D>(deserializer: D) -> Result<Address, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    Address::from_user_friendly_address(&s).map_err(|e| Error::custom(format!("{:?}", e)))
}

pub fn deserialize_nimiq_address_opt<'de, D>(deserializer: D) -> Result<Option<Address>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt: Option<String> = Deserialize::deserialize(deserializer)?;
    if let Some(s) = opt {
        Ok(Some(
            Address::from_user_friendly_address(&s)
                .map_err(|e| Error::custom(format!("{:?}", e)))?,
        ))
    } else {
        Ok(None)
    }
}

pub(crate) fn deserialize_coin<'de, D>(deserializer: D) -> Result<Coin, D::Error>
where
    D: Deserializer<'de>,
{
    let value: u64 = Deserialize::deserialize(deserializer)?;
    Coin::try_from(value).map_err(Error::custom)
}

pub(crate) fn deserialize_validator_id<'de, D>(deserializer: D) -> Result<ValidatorId, D::Error>
where
    D: Deserializer<'de>,
{
    let validator_id_hex: String = Deserialize::deserialize(deserializer)?;
    let validator_id_raw = hex::decode(validator_id_hex).map_err(Error::custom)?;
    ValidatorId::deserialize_from_vec(&validator_id_raw).map_err(Error::custom)
}

pub(crate) fn deserialize_bls_public_key<'de, D>(deserializer: D) -> Result<BlsPublicKey, D::Error>
where
    D: Deserializer<'de>,
{
    let pkey_hex: String = Deserialize::deserialize(deserializer)?;
    let pkey_raw = hex::decode(pkey_hex).map_err(Error::custom)?;
    BlsPublicKey::deserialize_from_vec(&pkey_raw).map_err(Error::custom)
}

pub(crate) fn deserialize_bls_secret_key_opt<'de, D>(
    deserializer: D,
) -> Result<Option<BlsSecretKey>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt: Option<String> = Deserialize::deserialize(deserializer)?;
    if let Some(skey_hex) = opt {
        let skey_raw = hex::decode(skey_hex).map_err(Error::custom)?;
        Ok(Some(
            BlsSecretKey::deserialize_from_vec(&skey_raw).map_err(Error::custom)?,
        ))
    } else {
        Ok(None)
    }
}

pub(crate) fn deserialize_nimiq_hash<'de, D>(deserializer: D) -> Result<AnyHash, D::Error>
where
    D: Deserializer<'de>,
{
    let hash_hex: String = Deserialize::deserialize(deserializer)?;
    let foo = Vec::from_hex(hash_hex).map_err(|_e|serde::de::Error::custom("Unable to parse hash"))?;
    Ok(AnyHash::from(&foo[..]))
}

pub(crate) fn deserialize_nimiq_hash_algorithm<'de, D>(deserializer: D) -> Result<HashAlgorithm, D::Error>
where
    D: Deserializer<'de>,
{
    let hash_algorithm: String = Deserialize::deserialize(deserializer)?;
    match hash_algorithm.as_str() {
        "blake2b" => Ok(HashAlgorithm::Blake2b),
        "sha256" => Ok(HashAlgorithm::Sha256),
        _ => Err(Error::custom("Unexpected hash algorithm")),
    }
}