use std::fmt;

use thiserror::Error;

use beserial::{Deserialize, Serialize};
use nimiq_collections::bitset::BitSet;
use nimiq_hash::{Blake2bHash, Blake2sHash, Hash, SerializeContent};
use nimiq_hash_derive::SerializeContent;
use nimiq_nano_primitives::MacroBlock as ZKPMacroBlock;
use nimiq_nano_primitives::{pk_tree_construct, PK_TREE_BREADTH};
use nimiq_primitives::policy::Policy;
use nimiq_primitives::slots::Validators;
use nimiq_vrf::VrfSeed;

use crate::signed::{Message, PREFIX_TENDERMINT_PROPOSAL};
use crate::tendermint::TendermintProof;
use crate::BlockError;

/// The struct representing a Macro block (can be either checkpoint or election).
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct MacroBlock {
    /// The header, contains some basic information and commitments to the body and the state.
    pub header: MacroHeader,
    /// The body of the block.
    pub body: Option<MacroBody>,
    /// The justification, contains all the information needed to verify that the header was signed
    /// by the correct producers.
    pub justification: Option<TendermintProof>,
}

impl MacroBlock {
    /// Returns the Blake2b hash of the block header.
    pub fn hash(&self) -> Blake2bHash {
        self.header.hash()
    }

    /// Returns the Blake2s hash of the block header.
    pub fn hash_blake2s(&self) -> Blake2sHash {
        self.header.hash()
    }

    /// Calculates the following function:
    ///     nano_zkp_hash = Blake2s( Blake2b(header) || pk_tree_root )
    /// Where `pk_tree_root` is the root of a special Merkle tree containing the BLS public keys of
    /// the validators for the next epoch.
    /// The `pk_tree_root` is necessary for the Nano ZK proofs and needs to be inserted into the
    /// signature for the macro blocks. The easiest way is to calculate this modified hash and then
    /// use it as the signature message.
    /// Also, the final hash is done with Blake2s because the ZKP circuits can only handle Blake2s.
    /// Only election blocks have the `validators` field, which contain the validators for the next
    /// epoch, so for checkpoint blocks the `pk_tree_root` doesn't exist. Then, for checkpoint blocks
    /// this function simply returns:
    ///     nano_zkp_hash = Blake2s( Blake2b(header) )
    pub fn nano_zkp_hash(&self, recalculate_pk_tree: bool) -> Blake2sHash {
        let mut message = self.hash().serialize_to_vec();

        if let Some(validators) = self.get_validators() {
            // Create the tree.
            let mut pk_tree_root = self.get_pk_tree_root();
            if recalculate_pk_tree || pk_tree_root.is_none() {
                pk_tree_root = MacroBlock::pk_tree_root(&validators).ok();
            }
            if let Some(mut pk_tree_root) = pk_tree_root {
                // Add it to the message.
                message.append(&mut pk_tree_root);
            }
        }

        // Return the final hash.
        message.hash()
    }

    fn get_pk_tree_root(&self) -> Option<Vec<u8>> {
        self.body.as_ref()?.pk_tree_root.clone()
    }

    /// Calculates the PKTree root from the given validators.
    pub fn pk_tree_root(validators: &Validators) -> Result<Vec<u8>, BlockError> {
        // Get the public keys.
        let public_keys = validators.voting_keys();

        // Check the expected number of validators.
        // This must be checked before `pk_tree_construct` since it assumes a correct value.
        if public_keys.len() != Policy::SLOTS as usize || public_keys.len() % PK_TREE_BREADTH != 0 {
            warn!(
                num_pks = public_keys.len(),
                "Unexpected number of validator public keys"
            );
            return Err(BlockError::InvalidValidators);
        }

        // Create the tree
        Ok(pk_tree_construct(
            public_keys.iter().map(|pk| pk.public_key).collect(),
        ))
    }

    /// Returns whether or not this macro block is an election block.
    pub fn is_election_block(&self) -> bool {
        Policy::is_election_block_at(self.header.block_number)
    }

    /// Returns a copy of the validator slots. Only returns Some if it is an election block.
    pub fn get_validators(&self) -> Option<Validators> {
        self.body.as_ref()?.validators.clone()
    }

    /// Returns the block number of this macro block.
    pub fn block_number(&self) -> u32 {
        self.header.block_number
    }

    /// Return the round of this macro block.
    pub fn round(&self) -> u32 {
        self.header.round
    }

    /// Returns the epoch number of this macro block.
    pub fn epoch_number(&self) -> u32 {
        Policy::epoch_at(self.header.block_number)
    }

    /// Verifies that the block is valid for the given validators.
    pub(crate) fn verify_validators(&self, validators: &Validators) -> Result<(), BlockError> {
        // Verify the Tendermint proof.
        if !TendermintProof::verify(self, validators) {
            warn!(
                %self,
                reason = "Macro block with bad justification",
                "Rejecting block"
            );
            return Err(BlockError::InvalidJustification);
        }

        Ok(())
    }
}

impl<'a> TryFrom<&'a MacroBlock> for ZKPMacroBlock {
    type Error = ();

    fn try_from(block: &'a MacroBlock) -> Result<ZKPMacroBlock, Self::Error> {
        if let Some(proof) = block.justification.as_ref() {
            Ok(ZKPMacroBlock {
                block_number: block.block_number(),
                round_number: proof.round,
                header_hash: block.hash().into(),
                signature: proof.sig.signature.get_point(),
                signer_bitmap: proof
                    .sig
                    .signers
                    .iter_bits()
                    .take(Policy::SLOTS as usize)
                    .collect(),
            })
        } else {
            Ok(ZKPMacroBlock::without_signatures(
                block.block_number(),
                0,
                block.hash().into(),
            ))
        }
    }
}

impl fmt::Display for MacroBlock {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        fmt::Display::fmt(&self.header, f)
    }
}

/// The struct representing the header of a Macro block (can be either checkpoint or election).
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, SerializeContent)]
pub struct MacroHeader {
    /// The version number of the block. Changing this always results in a hard fork.
    pub version: u16,
    /// The number of the block.
    pub block_number: u32,
    /// The round number this block was proposed in.
    pub round: u32,
    /// The timestamp of the block. It follows the Unix time and has millisecond precision.
    pub timestamp: u64,
    /// The hash of the header of the immediately preceding block (either micro or macro).
    pub parent_hash: Blake2bHash,
    /// The hash of the header of the preceding election macro block.
    pub parent_election_hash: Blake2bHash,
    /// The seed of the block. This is the BLS signature of the seed of the immediately preceding
    /// block (either micro or macro) using the validator key of the block proposer.
    pub seed: VrfSeed,
    /// The extra data of the block. It is simply up to 32 raw bytes.
    ///
    /// It encodes the initial supply in the genesis block, as a big-endian `u64`.
    ///
    /// No planned use otherwise.
    #[beserial(len_type(u8, limit = 32))]
    pub extra_data: Vec<u8>,
    /// The root of the Merkle tree of the blockchain state. It just acts as a commitment to the
    /// state.
    pub state_root: Blake2bHash,
    /// The root of the Merkle tree of the body. It just acts as a commitment to the body.
    pub body_root: Blake2bHash,
    /// A merkle root over all of the transactions that happened in the current epoch.
    pub history_root: Blake2bHash,
}

impl Message for MacroHeader {
    const PREFIX: u8 = PREFIX_TENDERMINT_PROPOSAL;
}

#[allow(clippy::derive_hash_xor_eq)] // TODO: Shouldn't be necessary
impl Hash for MacroHeader {}

impl fmt::Display for MacroHeader {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(
            f,
            "#{}:MA:{}",
            self.block_number,
            self.hash::<Blake2bHash>().to_short_str(),
        )
    }
}

/// The struct representing the body of a Macro block (can be either checkpoint or election).
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, SerializeContent)]
pub struct MacroBody {
    /// Contains all the information regarding the next validator set, i.e. their validator
    /// public key, their reward address and their assigned validator slots.
    /// Is only Some when the macro block is an election block.
    pub validators: Option<Validators>,
    /// The root of a special Merkle tree over the next validator's voting keys. It is necessary to
    /// verify the zero-knowledge proofs used in the nano-sync.
    /// Is only Some when the macro block is an election block.
    #[beserial(len_type(u8, limit = 96))]
    pub pk_tree_root: Option<Vec<u8>>,
    /// A bitset representing which validator slots had their reward slashed at the time when this
    /// block was produced. It is used later on for reward distribution.
    pub lost_reward_set: BitSet,
    /// A bitset representing which validator slots were prohibited from producing micro blocks or
    /// proposing macro blocks at the time when this block was produced. It is used later on for
    /// reward distribution.
    pub disabled_set: BitSet,
}

impl MacroBody {
    pub(crate) fn verify(
        &self,
        is_election: bool,
        check_pk_tree_root: bool,
    ) -> Result<(), BlockError> {
        if is_election != self.validators.is_some() {
            return Err(BlockError::InvalidValidators);
        }

        if is_election != self.pk_tree_root.is_some() {
            return Err(BlockError::InvalidPkTreeRoot);
        }

        // If this is an election block and check_pk_tree_root is set,
        // check if the pk_tree_root matches the validators.
        if is_election && check_pk_tree_root {
            match MacroBlock::pk_tree_root(self.validators.as_ref().unwrap()) {
                Ok(pk_tree_root) => {
                    if pk_tree_root != *self.pk_tree_root.as_ref().unwrap() {
                        return Err(BlockError::InvalidPkTreeRoot);
                    }
                }
                Err(e) => {
                    warn!(error=%e, "PK tree root building failed");
                    return Err(e);
                }
            }
        }

        Ok(())
    }
}

#[allow(clippy::derive_hash_xor_eq)] // TODO: Shouldn't be necessary
impl Hash for MacroBody {}

#[derive(Error, Debug)]
pub enum IntoSlotsError {
    #[error("Body missing in macro block")]
    MissingBody,
    #[error("Not an election macro block")]
    NoElection,
}
