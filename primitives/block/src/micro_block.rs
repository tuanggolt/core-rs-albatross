use std::fmt::Debug;
use std::{fmt, io};

use beserial::{Deserialize, Serialize};
use nimiq_database::{FromDatabaseValue, IntoDatabaseValue};
use nimiq_hash::{Blake2bHash, Hash, SerializeContent};
use nimiq_hash_derive::SerializeContent;
use nimiq_keys::Signature;
use nimiq_primitives::policy;
use nimiq_transaction::Transaction;
use nimiq_vrf::VrfSeed;

use crate::fork_proof::ForkProof;
use crate::ViewChangeProof;

/// The struct representing a Micro block.
/// A Micro block, unlike a Macro block, doesn't contain any inherents (data that can be calculated
/// by full nodes but for syncing and for nano nodes some needs to be explicitly included).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MicroBlock {
    /// The header, contains some basic information and commitments to the body and the state.
    pub header: MicroHeader,
    /// The justification, contains all the information needed to verify that the header was signed
    /// by the correct producer.
    pub justification: Option<MicroJustification>,
    /// The body of the block.
    pub body: Option<MicroBody>,
}

/// The struct representing the header of a Micro block.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, SerializeContent)]
pub struct MicroHeader {
    /// The version number of the block. Changing this always results in a hard fork.
    pub version: u16,
    /// The number of the block.
    pub block_number: u32,
    /// The view number of this block. It increases whenever a view change happens and resets on
    /// every macro block.
    pub view_number: u32,
    /// The timestamp of the block. It follows the Unix time and has millisecond precision.
    pub timestamp: u64,
    /// The hash of the header of the immediately preceding block (either micro or macro).
    pub parent_hash: Blake2bHash,
    /// The seed of the block. This is the BLS signature of the seed of the immediately preceding
    /// block (either micro or macro) using the validator key of the block producer.
    pub seed: VrfSeed,
    /// The extra data of the block. It is simply 32 raw bytes. No planned use.
    #[beserial(len_type(u8, limit = 32))]
    pub extra_data: Vec<u8>,
    /// The root of the Merkle tree of the blockchain state. It just acts as a commitment to the
    /// state.
    pub state_root: Blake2bHash,
    /// The root of the Merkle tree of the body. It just acts as a commitment to the
    /// body.
    pub body_root: Blake2bHash,
    /// A merkle root over all of the transactions that happened in the current epoch.
    pub history_root: Blake2bHash,
}

/// The struct representing the justification for a Micro block.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MicroJustification {
    /// The signature of the block producer.
    pub signature: Signature,
    /// The view change proof. It consists of the aggregated signatures to a single view change
    /// message. It is an Option since a view change might not occur for any given block.
    pub view_change_proof: Option<ViewChangeProof>,
}

/// The struct representing the body of a Micro block.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, SerializeContent)]
pub struct MicroBody {
    /// A vector containing the fork proofs for this block. It might be empty.
    #[beserial(len_type(u16))]
    pub fork_proofs: Vec<ForkProof>,
    /// A vector containing the transactions for this block. It might be empty.
    #[beserial(len_type(u16))]
    pub transactions: Vec<Transaction>,
}

impl MicroBlock {
    /// Returns the hash of the block header.
    pub fn hash(&self) -> Blake2bHash {
        self.header.hash()
    }

    // Returns the available size, in bytes, in a micro block body for transactions.
    pub fn get_available_bytes(num_fork_proofs: usize) -> usize {
        policy::MAX_SIZE_MICRO_BODY
            - (/*fork_proofs vector length*/2 + num_fork_proofs * ForkProof::SIZE
            + /*transactions vector length*/ 2)
    }
}

impl MicroHeader {
    /// Returns the size, in bytes, of a Micro block header. This represents the maximum possible
    /// size since we assume that the extra_data field is completely filled.
    pub const MAX_SIZE: usize =
        /*version*/
        2 + /*block_number*/ 4 + /*view_number*/ 4 + /*timestamp*/ 8
            + /*parent_hash*/ 32 + /*seed*/ VrfSeed::SIZE + /*extra_data*/ 32 +
            /*state_root*/ 32 + /*body_root*/ 32 + /*history_root*/ 32;
}

impl IntoDatabaseValue for MicroBlock {
    fn database_byte_size(&self) -> usize {
        self.serialized_size()
    }

    fn copy_into_database(&self, mut bytes: &mut [u8]) {
        Serialize::serialize(&self, &mut bytes).unwrap();
    }
}

impl FromDatabaseValue for MicroBlock {
    fn copy_from_database(bytes: &[u8]) -> io::Result<Self>
    where
        Self: Sized,
    {
        let mut cursor = io::Cursor::new(bytes);
        Ok(Deserialize::deserialize(&mut cursor)?)
    }
}

impl fmt::Display for MicroBlock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.header, f)
    }
}

impl Hash for MicroHeader {}

impl fmt::Display for MicroHeader {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(
            f,
            "#{}.{}:MI:{}",
            self.block_number,
            self.view_number,
            self.hash::<Blake2bHash>().to_short_str(),
        )
    }
}

impl Hash for MicroBody {}

impl Debug for MicroBody {
    fn fmt(&self, f: &mut fmt::Formatter) -> std::fmt::Result {
        let mut dbg = f.debug_struct("MicroBody");
        dbg.field("num_fork_proofs", &self.fork_proofs.len());
        dbg.field("num_transactions", &self.transactions.len());
        dbg.finish()
    }
}
