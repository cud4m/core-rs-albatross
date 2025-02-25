use std::error::Error;
use std::future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures::{stream, FutureExt, Stream, StreamExt};
use nimiq_block::{Block, MacroBlock};
use nimiq_blockchain::Blockchain;
use nimiq_blockchain_interface::{AbstractBlockchain, BlockchainEvent, Direction};
use nimiq_genesis::NetworkInfo;
use nimiq_nano_primitives::state_commitment;
use nimiq_network_interface::network::Network;
use nimiq_primitives::policy::Policy;
use parking_lot::lock_api::RwLockUpgradableReadGuard;
use parking_lot::{RwLock, RwLockWriteGuard};

use tokio::sync::broadcast::{channel as broadcast, Sender as BroadcastSender};

use crate::proof_gen_utils::*;
use crate::types::*;
use crate::zkp_component::BROADCAST_MAX_CAPACITY;

/// ZK Prover generates the zk proof for an election block. It has:
///
/// - The network
/// - The current zkp state
/// - The channel to kill the current process generating the proof
/// - The stream with the generated zero knowledge proofs
///
/// The proofs are returned by polling the components.
pub struct ZKProver<N: Network> {
    network: Arc<N>,
    zkp_state: Arc<RwLock<ZKPState>>,
    sender: BroadcastSender<()>,
    proof_stream:
        Pin<Box<dyn Stream<Item = Result<(ZKPState, MacroBlock), ZKProofGenerationError>> + Send>>,
}

impl<N: Network> ZKProver<N> {
    pub async fn new(
        blockchain: Arc<RwLock<Blockchain>>,
        network: Arc<N>,
        zkp_state: Arc<RwLock<ZKPState>>,
        prover_path: Option<PathBuf>,
        keys_path: PathBuf,
    ) -> Self {
        let network_info = NetworkInfo::from_network_id(blockchain.read().network_id());
        let genesis_block = network_info.genesis_block::<Block>().unwrap_macro();

        let public_keys = zkp_state.read().latest_pks.clone();
        let genesis_state = state_commitment(
            genesis_block.block_number(),
            genesis_block.hash().into(),
            public_keys,
        );

        // Gets the stream of blockchain events and converts it into an election macro block stream
        let blockchain_election_rx = blockchain.read().notifier_as_stream();
        let blockchain2 = Arc::clone(&blockchain);

        let blockchain_election_rx = blockchain_election_rx.filter_map(move |event| {
            let result = match event {
                BlockchainEvent::EpochFinalized(hash) => {
                    let block = blockchain2.read().get_block(&hash, true, None);
                    if let Ok(Block::Macro(block)) = block {
                        Some(block)
                    } else {
                        None
                    }
                }
                _ => None,
            };
            future::ready(result)
        });

        // Prepends the election blocks from the blockchain for which we don't have a proof yet
        let blockchain_rg = blockchain.read();
        let current_state_height = zkp_state.read().latest_block_number;
        let blockchain_election_height = blockchain_rg.state.election_head.block_number();

        let blocks = if blockchain_election_height > current_state_height {
            blockchain_rg
                .get_macro_blocks(
                    &zkp_state.read().latest_header_hash,
                    (blockchain_election_height - current_state_height)
                        / Policy::blocks_per_epoch(),
                    true,
                    Direction::Forward,
                    true,
                    None,
                )
                .expect("Fetching election blocks for zkp prover initialization failed")
                .drain(..)
                .map(|block| block.unwrap_macro())
                .collect()
        } else {
            vec![]
        };
        let blockchain_election_rx = stream::iter(blocks).chain(blockchain_election_rx);
        drop(blockchain_rg);

        // Upon every election block, a proof generation process is launched.
        // The assertion holds true since we should only start generating the next proof after its predecessor's
        // proof has been pushed into our state.
        //
        // Note: The election block stream may have blocks that are too old relative to our zkp state;
        // thus we will filter those blocks out.
        let (sender, recv) = broadcast(BROADCAST_MAX_CAPACITY);
        let zkp_state2 = Arc::clone(&zkp_state);
        let proof_stream = blockchain_election_rx
            .filter_map(move |block| {
                let genesis_state3 = genesis_state.clone();
                let zkp_state3 = Arc::clone(&zkp_state2);
                let recv = recv.resubscribe();
                let zkp_state = zkp_state3.read();
                let prover_path = prover_path.clone();
                let keys_path2 = keys_path.clone();
                assert!(
                    zkp_state.latest_block_number
                        >= block.block_number() - Policy::blocks_per_epoch(),
                    "The current state (block height: {}) should never lag behind more than one epoch. Current height: {}",
                    zkp_state.latest_block_number,
                    block.block_number(),
                );
                if zkp_state.latest_block_number
                    == block.block_number() - Policy::blocks_per_epoch()
                {
                    launch_generate_new_proof(
                        recv,
                        ProofInput {
                            block: block.clone(),
                            latest_pks: zkp_state.latest_pks.clone(),
                            latest_header_hash: zkp_state.latest_header_hash.clone(),
                            previous_proof: zkp_state.latest_proof.clone(),
                            genesis_state: genesis_state3,
                            keys_path: keys_path2,
                        },
                        prover_path,
                    )
                    .map(|res| Some(res.map(|state| (state, block))))
                    .left_future()
                } else {
                    future::ready(None).right_future()
                }
            })
            .boxed();

        Self {
            network,
            zkp_state,
            sender,
            proof_stream,
        }
    }

    /// The broadcasting of the generated zk proof.
    fn broadcast_zk_proof(network: &Arc<N>, zk_proof: ZKProof) {
        let network = Arc::clone(network);
        tokio::spawn(async move {
            if let Err(e) = network.publish::<ZKProofTopic>(zk_proof).await {
                log::warn!(error = &e as &dyn Error, "Failed to publish the zk proof");
            }
        });
    }

    /// This sends the kill signal to the proof generation process.
    pub(crate) fn cancel_current_proof_production(&mut self) {
        self.sender.send(()).unwrap();
    }
}

impl<N: Network> Stream for ZKProver<N> {
    type Item = (ZKProof, MacroBlock);

    fn poll_next(mut self: Pin<&mut ZKProver<N>>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        // If a new proof was generated it sets the state and broadcasts the new proof.
        while let Poll::Ready(Some(proof)) = self.proof_stream.poll_next_unpin(cx) {
            match proof {
                Ok((new_zkp_state, block)) => {
                    assert!(
                        new_zkp_state.latest_proof.is_some(),
                        "The generate new proof should never produces a empty proof"
                    );
                    let zkp_state_lock = self.zkp_state.upgradable_read();

                    // If we received a more recent proof in the meanwhile, we should have cancelled the proof generation process already.
                    assert!(
                        zkp_state_lock.latest_block_number < new_zkp_state.latest_block_number,
                        "The generated proof should always be more recent than the current state"
                    );

                    let mut zkp_state_lock = RwLockUpgradableReadGuard::upgrade(zkp_state_lock);
                    *zkp_state_lock = new_zkp_state;

                    let zkp_state_lock = RwLockWriteGuard::downgrade(zkp_state_lock);

                    let proof: ZKProof = zkp_state_lock.clone().into();
                    Self::broadcast_zk_proof(&self.network, proof.clone());
                    return Poll::Ready(Some((proof, block)));
                }
                Err(e) => {
                    log::error!("Error generating ZK Proof for block {}", e);
                }
            };
        }

        Poll::Pending
    }
}
