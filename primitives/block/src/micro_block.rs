use std::cmp::Ordering;
use std::collections::HashSet;
use std::fmt::Debug;
use std::{fmt, io};

use crate::{BlockError, SkipBlockInfo};
use beserial::{Deserialize, Serialize};
use nimiq_database_value::{FromDatabaseValue, IntoDatabaseValue};
use nimiq_hash::{Blake2bHash, Hash, SerializeContent};
use nimiq_hash_derive::SerializeContent;
use nimiq_keys::{PublicKey, Signature};
use nimiq_primitives::policy::Policy;
use nimiq_primitives::slots::Validators;
use nimiq_transaction::ExecutedTransaction;
use nimiq_transaction::Transaction;
use nimiq_vrf::VrfSeed;

use crate::fork_proof::ForkProof;
use crate::skip_block::SkipBlockProof;

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

impl MicroBlock {
    /// Returns the hash of the block header.
    pub fn hash(&self) -> Blake2bHash {
        self.header.hash()
    }

    /// Returns whether the micro block is a skip block
    pub fn is_skip_block(&self) -> bool {
        if let Some(justification) = &self.justification {
            return matches!(justification, MicroJustification::Skip(_));
        }
        false
    }

    /// Returns the available size, in bytes, in a micro block body for transactions.
    pub fn get_available_bytes(num_fork_proofs: usize) -> usize {
        Policy::MAX_SIZE_MICRO_BODY
            - (/*fork_proofs vector length*/2 + num_fork_proofs * ForkProof::SIZE
            + /*transactions vector length*/ 2)
    }

    pub(crate) fn verify_proposer(&self, signing_key: &PublicKey) -> Result<(), BlockError> {
        let justification = self
            .justification
            .as_ref()
            .ok_or(BlockError::MissingJustification)?;

        match justification {
            MicroJustification::Micro(signature) => {
                let hash = self.hash();
                if !signing_key.verify(signature, hash.as_slice()) {
                    warn!(
                        block = %self,
                        %signing_key,
                        reason = "Invalid signature for proposer",
                        "Rejecting block"
                    );
                    return Err(BlockError::InvalidJustification);
                }
            }
            MicroJustification::Skip(_) => {}
        };

        Ok(())
    }

    pub(crate) fn verify_validators(&self, validators: &Validators) -> Result<(), BlockError> {
        let justification = self
            .justification
            .as_ref()
            .ok_or(BlockError::MissingJustification)?;

        match justification {
            MicroJustification::Skip(proof) => {
                let skip_block = SkipBlockInfo {
                    block_number: self.header.block_number,
                    vrf_entropy: self.header.seed.entropy(),
                };

                if !proof.verify(&skip_block, validators) {
                    debug!(
                        block = %self,
                        reason = "Bad skip block proof",
                        "Rejecting block"
                    );
                    return Err(BlockError::InvalidSkipBlockProof);
                }
            }
            MicroJustification::Micro(_) => {}
        }

        Ok(())
    }
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

/// Enumeration representing the justification for a Micro block
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "serde-derive", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde-derive", serde(rename_all = "camelCase"))]
#[repr(u8)]
pub enum MicroJustification {
    /// Regular micro block justification which is the signature of the block producer
    Micro(Signature),
    /// Skip block justification which is the aggregated signature of the validator's
    /// signatures for a skip block.
    Skip(SkipBlockProof),
}

impl MicroJustification {
    pub fn unwrap_micro(self) -> Signature {
        match self {
            MicroJustification::Micro(signature) => signature,
            MicroJustification::Skip(_) => unreachable!(),
        }
    }
}
/// The struct representing the header of a Micro block.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, SerializeContent)]
pub struct MicroHeader {
    /// The version number of the block. Changing this always results in a hard fork.
    pub version: u16,
    /// The number of the block.
    pub block_number: u32,
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
    /// A Merkle root over all of the transactions that happened in the current epoch.
    pub history_root: Blake2bHash,
}

impl MicroHeader {
    /// Returns the size, in bytes, of a Micro block header. This represents the maximum possible
    /// size since we assume that the extra_data field is completely filled.
    pub const MAX_SIZE: usize =
        /*version*/
        2 + /*block_number*/ 4 + /*timestamp*/ 8 + /*parent_hash*/ 32
            + /*seed*/ VrfSeed::SIZE + /*extra_data*/ 32 + /*state_root*/ 32
            + /*body_root*/ 32 + /*history_root*/ 32;
}

impl Hash for MicroHeader {}

impl fmt::Display for MicroHeader {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(
            f,
            "#{}:MI:{}",
            self.block_number,
            self.hash::<Blake2bHash>().to_short_str(),
        )
    }
}

/// The struct representing the body of a Micro block.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, SerializeContent)]
pub struct MicroBody {
    /// A vector containing the fork proofs for this block. It might be empty.
    #[beserial(len_type(u16))]
    pub fork_proofs: Vec<ForkProof>,
    /// A vector containing the transactions for this block. It might be empty.
    #[beserial(len_type(u16))]
    pub transactions: Vec<ExecutedTransaction>,
}

impl MicroBody {
    pub fn get_raw_transactions(&self) -> Vec<Transaction> {
        // Extract the transactions from the block
        self.transactions
            .iter()
            .map(|txn| txn.get_raw_transaction().clone())
            .collect()
    }

    pub(crate) fn verify(&self, is_skip: bool, block_number: u32) -> Result<(), BlockError> {
        // Check that the maximum body size is not exceeded.
        let body_size = self.serialized_size();
        if body_size > Policy::MAX_SIZE_MICRO_BODY {
            debug!(
                body_size = body_size,
                max_size = Policy::MAX_SIZE_MICRO_BODY,
                reason = "Body size exceeds maximum size",
                "Invalid block"
            );
            return Err(BlockError::SizeExceeded);
        }

        // Check that the body is empty for skip blocks.
        if is_skip && (!self.fork_proofs.is_empty() || !self.transactions.is_empty()) {
            debug!(
                num_transactions = self.transactions.len(),
                num_fork_proofs = self.fork_proofs.len(),
                reason = "Skip block has a non empty body",
                "Invalid block"
            );
            return Err(BlockError::InvalidSkipBlockBody);
        }

        // Ensure that fork proofs are ordered, unique and within their reporting window.
        let mut previous_proof: Option<&ForkProof> = None;
        for proof in &self.fork_proofs {
            // Check reporting window.
            if !proof.is_valid_at(block_number) {
                return Err(BlockError::InvalidForkProof);
            }

            // Check proof ordering and uniqueness.
            if let Some(previous) = previous_proof {
                match previous.cmp(proof) {
                    Ordering::Equal => {
                        return Err(BlockError::DuplicateForkProof);
                    }
                    Ordering::Greater => {
                        return Err(BlockError::ForkProofsNotOrdered);
                    }
                    _ => {}
                }
            }
            previous_proof = Some(proof);
        }

        // Ensure transactions are unique and within their validity window.
        let mut uniq = HashSet::new();
        for tx in &self.get_raw_transactions() {
            // Check validity window.
            if !tx.is_valid_at(block_number) {
                return Err(BlockError::ExpiredTransaction);
            }

            // Check uniqueness.
            if !uniq.insert(tx.hash::<Blake2bHash>()) {
                return Err(BlockError::DuplicateTransaction);
            }
        }

        Ok(())
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
