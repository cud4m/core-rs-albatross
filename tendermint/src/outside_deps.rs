use crate::state::TendermintState;
use crate::utils::{AggregationResult, ProposalResult, Step, TendermintError};
use crate::{ProofTrait, ProposalCacheTrait, ProposalHashTrait, ProposalTrait, ResultTrait};
use async_trait::async_trait;
use futures::future::BoxFuture;

/// The (async) trait that we need for all of Tendermint's low-level functions. The functions are
/// mostly about producing proposals and networking.
#[async_trait]
pub trait TendermintOutsideDeps: Send + Unpin + Sized + 'static {
    type ProposalTy: ProposalTrait;
    type ProposalHashTy: ProposalHashTrait;
    type ProposalCacheTy: ProposalCacheTrait;
    type ProofTy: ProofTrait;
    type ResultTy: ResultTrait;

    /// The block height this Tendermint instance is supposed to be used for.
    fn block_height(&self) -> u32;

    /// The round Tendermint is supposed to start with.
    fn initial_round(&self) -> u32;

    /// Verify that a given Tendermint state is valid. This is necessary when we are initializing
    /// using a previous state.
    fn verify_state(
        &self,
        state: &TendermintState<
            Self::ProposalTy,
            Self::ProposalCacheTy,
            Self::ProposalHashTy,
            Self::ProofTy,
        >,
    ) -> bool;

    /// Checks if it our turn to propose for the given round.
    fn is_our_turn(&self, round: u32) -> bool;

    /// Produces a proposal for the given round. It is used when it is our turn to propose. The
    /// proposal is guaranteed to be valid.
    fn get_value(
        &mut self,
        round: u32,
    ) -> Result<(Self::ProposalTy, Self::ProposalCacheTy), TendermintError>;

    /// Takes a proposal and a proof (2f+1 precommits) and returns a completed block.
    fn assemble_block(
        &self,
        round: u32,
        proposal: Self::ProposalTy,
        proposal_cache: Option<Self::ProposalCacheTy>,
        proof: Self::ProofTy,
    ) -> Result<Self::ResultTy, TendermintError>;

    /// Broadcasts a proposal message (which includes the proposal and the proposer's valid round).
    /// This is a Future and it is allowed to fail.
    async fn broadcast_proposal(
        self,
        round: u32,
        proposal: Self::ProposalTy,
        valid_round: Option<u32>,
    ) -> Result<Self, TendermintError>;

    /// Waits for a proposal message (which includes the proposal and the proposer's valid round).
    /// The received proposal (if any) is guaranteed to be valid. This function also has to take
    /// care of waiting before timing out.
    /// This is a Future and it is allowed to fail.
    async fn await_proposal(
        self,
        round: u32,
    ) -> Result<
        (
            Self,
            ProposalResult<Self::ProposalTy, Self::ProposalCacheTy>,
        ),
        TendermintError,
    >;

    /// Broadcasts a vote (either prevote or precommit) for a given round and proposal. It then
    /// returns an aggregation of the 2f+1 votes received from other nodes for this round (and
    /// corresponding step).
    /// It also has to take care of waiting before timing out.
    /// This is a Future and it is allowed to fail.
    async fn broadcast_and_aggregate(
        self,
        round: u32,
        step: Step,
        proposal_hash: Option<Self::ProposalHashTy>,
    ) -> Result<(Self, AggregationResult<Self::ProposalHashTy, Self::ProofTy>), TendermintError>;

    /// Rebroadcasts a vote and starts an aggregations, but does not wait for the result.
    /// Used for restarting aggregations from state.
    fn rebroadcast_and_aggregate(
        &self,
        round: u32,
        step: Step,
        proposal_hash: Option<Self::ProposalHashTy>,
    );

    /// Returns the current aggregation for a given round and step. The returned aggregation might
    /// or not have 2f+1 votes, this function only returns all the votes that we have so far.
    /// It will fail if no aggregation was started for the given round and step.
    /// This is a Future and it is allowed to fail.
    async fn get_aggregation(
        self,
        round: u32,
        step: Step,
    ) -> Result<(Self, AggregationResult<Self::ProposalHashTy, Self::ProofTy>), TendermintError>;

    // Calculates the hash of a given proposal. We have it into a separate function so that
    // we can support arbitrary hashing schemes.
    fn hash_proposal(
        &self,
        proposal: Self::ProposalTy,
        proposal_cache: Self::ProposalCacheTy,
    ) -> Self::ProposalHashTy;

    fn get_background_task(&mut self) -> BoxFuture<'static, ()>;
}
