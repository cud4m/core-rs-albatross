use std::sync::Arc;

use parking_lot::RwLock;
use rand::CryptoRng;
use rand::Rng;

use nimiq_block::Block;
use nimiq_block_production::BlockProducer;
use nimiq_blockchain::Blockchain;
use nimiq_blockchain_interface::{AbstractBlockchain, PushResult};
use nimiq_primitives::policy::Policy;

use crate::blockchain::sign_macro_block;

/// Produces a series of macro blocks (and the corresponding batches).
pub fn produce_macro_blocks_with_rng<R: Rng + CryptoRng>(
    producer: &BlockProducer,
    blockchain: &Arc<RwLock<Blockchain>>,
    num_blocks: usize,
    rng: &mut R,
) {
    for _ in 0..num_blocks {
        fill_micro_blocks_with_rng(producer, blockchain, rng);

        let blockchain = blockchain.upgradable_read();
        let next_block_height = (blockchain.block_number() + 1) as u64;

        let macro_block_proposal = producer.next_macro_block_proposal_with_rng(
            &blockchain,
            blockchain.head().timestamp() + next_block_height * 1000,
            0u32,
            vec![],
            rng,
        );

        let block = sign_macro_block(
            &producer.voting_key,
            macro_block_proposal.header,
            macro_block_proposal.body,
        );

        assert_eq!(
            Blockchain::push(blockchain, Block::Macro(block)),
            Ok(PushResult::Extended)
        );
    }
}

/// Create the next micro block with default parameters.
pub fn next_micro_block_with_rng<R: Rng + CryptoRng>(
    producer: &BlockProducer,
    blockchain: &Arc<RwLock<Blockchain>>,
    rng: &mut R,
) -> Block {
    let blockchain = blockchain.upgradable_read();
    let block = producer.next_micro_block_with_rng(
        &blockchain,
        blockchain.head().timestamp() + 500,
        vec![],
        vec![],
        vec![0x42],
        None,
        rng,
    );
    Block::Micro(block)
}

/// Creates and pushes a single micro block to the chain.
pub fn push_micro_block_with_rng<R: Rng + CryptoRng>(
    producer: &BlockProducer,
    blockchain: &Arc<RwLock<Blockchain>>,
    rng: &mut R,
) -> Block {
    let block = next_micro_block_with_rng(producer, blockchain, rng);
    let blockchain = blockchain.upgradable_read();
    assert_eq!(
        Blockchain::push(blockchain, block.clone()),
        Ok(PushResult::Extended)
    );
    block
}

/// Fill batch with micro blocks.
pub fn fill_micro_blocks_with_rng<R: Rng + CryptoRng>(
    producer: &BlockProducer,
    blockchain: &Arc<RwLock<Blockchain>>,
    rng: &mut R,
) {
    let init_height = blockchain.read().block_number();

    assert!(Policy::is_macro_block_at(init_height));

    let macro_block_number = init_height + Policy::blocks_per_batch();

    for _ in (init_height + 1)..macro_block_number {
        push_micro_block_with_rng(producer, blockchain, rng);
    }

    assert_eq!(blockchain.read().block_number(), macro_block_number - 1);
}
