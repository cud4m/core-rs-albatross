use std::fmt::Debug;

use beserial::{Deserialize, Serialize};
use nimiq_bls::AggregatePublicKey;
use nimiq_hash::{Hash, SerializeContent};
use nimiq_hash_derive::SerializeContent;
use nimiq_primitives::policy::Policy;
use nimiq_primitives::slots::Validators;
use nimiq_vrf::VrfEntropy;

use crate::{Message, MultiSignature, SignedMessage, PREFIX_SKIP_BLOCK_INFO};

pub type SignedSkipBlockInfo = SignedMessage<SkipBlockInfo>;

/// A struct that represents the basic information of a skip block.
#[derive(
    Clone, Debug, Ord, PartialOrd, Eq, PartialEq, Deserialize, Serialize, SerializeContent,
)]
pub struct SkipBlockInfo {
    /// The number of the block for which the skip block is constructed.
    pub block_number: u32,

    /// The seed of the previous block. This is needed to distinguish skip blocks on different
    /// branches. We chose the seed so that the skip block applies to all branches of a malicious
    /// fork, but not to branching because of skip blocks.
    /// We use the seed entropy since that is what is actually unique, not the VRF seed itself.
    pub vrf_entropy: VrfEntropy,
}

impl Message for SkipBlockInfo {
    const PREFIX: u8 = PREFIX_SKIP_BLOCK_INFO;
}

impl Hash for SkipBlockInfo {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "serde-derive", derive(serde::Serialize, serde::Deserialize))]
pub struct SkipBlockProof {
    // The aggregated signature of the validator's signatures for the skip block.
    pub sig: MultiSignature,
}

impl SkipBlockProof {
    /// Verifies the proof. This only checks that the proof is valid for this skip block, not that
    /// the skip block itself is valid.
    pub fn verify(&self, skip_block: &SkipBlockInfo, validators: &Validators) -> bool {
        // Check if there are enough votes.
        if self.sig.signers.len() < Policy::TWO_F_PLUS_ONE as usize {
            error!(
                "SkipBlockProof verification failed: Not enough slots signed the skip block message."
            );
            return false;
        }

        // Get the public key for each SLOT present in the signature and add them together to get
        // the aggregated public key.
        let agg_pk =
            self.sig
                .signers
                .iter()
                .fold(AggregatePublicKey::new(), |mut aggregate, slot| {
                    let pk = validators
                        .get_validator_by_slot_number(slot as u16)
                        .voting_key
                        .uncompress()
                        .expect("Failed to uncompress CompressedPublicKey");
                    aggregate.aggregate(&pk);
                    aggregate
                });

        // Verify the aggregated signature against our aggregated public key.
        agg_pk.verify_hash(skip_block.hash_with_prefix(), &self.sig.signature)
    }
}
