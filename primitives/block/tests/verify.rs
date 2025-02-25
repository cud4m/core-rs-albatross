use nimiq_block::{
    Block, BlockError, BlockHeader, ForkProof, MacroBlock, MacroBody, MacroHeader, MicroBlock,
    MicroBody, MicroHeader, MicroJustification, MultiSignature, SkipBlockProof,
};
use nimiq_bls::AggregateSignature;
use nimiq_collections::BitSet;
use nimiq_hash::{Blake2bHash, Hash};
use nimiq_keys::{KeyPair, Signature};
use nimiq_primitives::{networks::NetworkId, policy::Policy, slots::Validators};
use nimiq_test_log::test;
use nimiq_test_utils::blockchain::generate_transactions;
use nimiq_transaction::ExecutedTransaction;
use nimiq_vrf::VrfSeed;

#[test]
fn test_verify_header_version() {
    let mut micro_header = MicroHeader {
        version: Policy::VERSION - 1,
        block_number: 1,
        timestamp: 0,
        parent_hash: Blake2bHash::default(),
        seed: VrfSeed::default(),
        extra_data: [].to_vec(),
        state_root: Blake2bHash::default(),
        body_root: Blake2bHash::default(),
        history_root: Blake2bHash::default(),
    };
    let header = BlockHeader::Micro(micro_header.clone());

    // Check version at header level
    assert_eq!(header.verify(false), Err(BlockError::UnsupportedVersion));

    let block = Block::Micro(MicroBlock {
        header: micro_header.clone(),
        justification: None,
        body: None,
    });

    // Error should remain at block level
    assert_eq!(block.verify(false), Err(BlockError::UnsupportedVersion));

    // Fix the version and check that it passes
    micro_header.version = Policy::VERSION;
    let header = BlockHeader::Micro(micro_header);
    assert_eq!(header.verify(false), Ok(()));
}

#[test]
fn test_verify_header_extra_data() {
    let mut micro_header = MicroHeader {
        version: Policy::VERSION,
        block_number: 1,
        timestamp: 0,
        parent_hash: Blake2bHash::default(),
        seed: VrfSeed::default(),
        extra_data: vec![0; 33],
        state_root: Blake2bHash::default(),
        body_root: Blake2bHash::default(),
        history_root: Blake2bHash::default(),
    };
    let header = BlockHeader::Micro(micro_header.clone());

    // Check extra data field at header level
    assert_eq!(header.verify(false), Err(BlockError::ExtraDataTooLarge));
    // Error should remain for a skip block
    assert_eq!(header.verify(true), Err(BlockError::ExtraDataTooLarge));

    let block = Block::Micro(MicroBlock {
        header: micro_header.clone(),
        justification: None,
        body: None,
    });

    // Error should remain at block level
    assert_eq!(block.verify(false), Err(BlockError::ExtraDataTooLarge));

    // Fix the extra data field and check that it passes
    micro_header.extra_data = vec![0; 32];
    let header = BlockHeader::Micro(micro_header.clone());
    assert_eq!(header.verify(false), Ok(()));
    // Error should remain for a skip block
    assert_eq!(header.verify(true), Err(BlockError::ExtraDataTooLarge));

    // Fix the extra data field for a skip block and check that it passes
    micro_header.extra_data = [].to_vec();
    let header = BlockHeader::Micro(micro_header);
    assert_eq!(header.verify(true), Ok(()));
}

#[test]
fn test_verify_body_root() {
    let mut micro_header = MicroHeader {
        version: Policy::VERSION,
        block_number: 1,
        timestamp: 0,
        parent_hash: Blake2bHash::default(),
        seed: VrfSeed::default(),
        extra_data: vec![0; 30],
        state_root: Blake2bHash::default(),
        body_root: Blake2bHash::default(),
        history_root: Blake2bHash::default(),
    };

    let micro_justification = MicroJustification::Micro(Signature::default());

    let micro_body = MicroBody {
        fork_proofs: [].to_vec(),
        transactions: [].to_vec(),
    };

    // Build a block with body
    let block = Block::Micro(MicroBlock {
        header: micro_header.clone(),
        justification: Some(micro_justification.clone()),
        body: Some(micro_body.clone()),
    });

    // The body root check must fail
    assert_eq!(block.verify(false), Err(BlockError::BodyHashMismatch));

    // Fix the body root and check that it passes
    micro_header.body_root = micro_body.hash();
    let block = Block::Micro(MicroBlock {
        header: micro_header.clone(),
        justification: Some(micro_justification),
        body: Some(micro_body),
    });

    assert_eq!(block.verify(false), Ok(()));
}

#[test]
fn test_verify_skip_block() {
    let mut micro_header = MicroHeader {
        version: Policy::VERSION,
        block_number: 1,
        timestamp: 0,
        parent_hash: Blake2bHash::default(),
        seed: VrfSeed::default(),
        extra_data: vec![],
        state_root: Blake2bHash::default(),
        body_root: Blake2bHash::default(),
        history_root: Blake2bHash::default(),
    };

    let micro_justification = MicroJustification::Skip(SkipBlockProof {
        sig: MultiSignature {
            signature: AggregateSignature::new(),
            signers: BitSet::default(),
        },
    });

    let transactions: Vec<ExecutedTransaction> =
        generate_transactions(&KeyPair::default(), 1, NetworkId::UnitAlbatross, 1, 0)
            .iter()
            .map(|tx| ExecutedTransaction::Ok(tx.clone()))
            .collect();

    let mut micro_body = MicroBody {
        fork_proofs: [].to_vec(),
        transactions: transactions,
    };

    // Build a block with body
    micro_header.body_root = micro_body.hash();
    let block = Block::Micro(MicroBlock {
        header: micro_header.clone(),
        justification: Some(micro_justification.clone()),
        body: Some(micro_body.clone()),
    });

    // The skip block body should fail
    assert_eq!(block.verify(false), Err(BlockError::InvalidSkipBlockBody));

    // Fix the body with empty transactions and check that it passes
    micro_body.transactions = vec![];
    micro_header.body_root = micro_body.hash();
    let block = Block::Micro(MicroBlock {
        header: micro_header.clone(),
        justification: Some(micro_justification),
        body: Some(micro_body),
    });

    assert_eq!(block.verify(false), Ok(()));
}

#[test]
fn test_verify_micro_block_body_txns() {
    let mut micro_header = MicroHeader {
        version: Policy::VERSION,
        block_number: 1,
        timestamp: 0,
        parent_hash: Blake2bHash::default(),
        seed: VrfSeed::default(),
        extra_data: vec![],
        state_root: Blake2bHash::default(),
        body_root: Blake2bHash::default(),
        history_root: Blake2bHash::default(),
    };

    let micro_justification = MicroJustification::Micro(Signature::default());

    let txns: Vec<ExecutedTransaction> =
        generate_transactions(&KeyPair::default(), 1, NetworkId::UnitAlbatross, 5, 0)
            .iter()
            .map(|tx| ExecutedTransaction::Ok(tx.clone()))
            .collect();

    // Lets have a duplicate transaction
    let mut txns_dup = txns.clone();
    txns_dup.push(txns.first().unwrap().clone());

    let mut micro_body = MicroBody {
        fork_proofs: [].to_vec(),
        transactions: txns_dup.clone(),
    };

    // Build a block with body with a duplicate transaction
    micro_header.body_root = micro_body.hash();
    let block = Block::Micro(MicroBlock {
        header: micro_header.clone(),
        justification: Some(micro_justification.clone()),
        body: Some(micro_body.clone()),
    });

    // The body check should fail
    assert_eq!(block.verify(false), Err(BlockError::DuplicateTransaction));

    // Fix the body with empty transactions and check that it passes
    txns_dup.pop();
    micro_body.transactions = txns_dup;
    micro_header.body_root = micro_body.hash();
    let block = Block::Micro(MicroBlock {
        header: micro_header.clone(),
        justification: Some(micro_justification.clone()),
        body: Some(micro_body),
    });

    assert_eq!(block.verify(false), Ok(()));

    // Now modify the validity start height
    let txns: Vec<ExecutedTransaction> = generate_transactions(
        &KeyPair::default(),
        Policy::blocks_per_epoch(),
        NetworkId::UnitAlbatross,
        5,
        0,
    )
    .iter()
    .map(|tx| ExecutedTransaction::Ok(tx.clone()))
    .collect();

    let micro_body = MicroBody {
        fork_proofs: [].to_vec(),
        transactions: txns.clone(),
    };

    // Build a block with body with the expired transactions
    micro_header.body_root = micro_body.hash();
    let block = Block::Micro(MicroBlock {
        header: micro_header.clone(),
        justification: Some(micro_justification),
        body: Some(micro_body.clone()),
    });

    // The body check should fail
    assert_eq!(block.verify(false), Err(BlockError::ExpiredTransaction));
}

#[test]
fn test_verify_micro_block_body_fork_proofs() {
    let mut micro_header = MicroHeader {
        version: Policy::VERSION,
        block_number: 1,
        timestamp: 0,
        parent_hash: Blake2bHash::default(),
        seed: VrfSeed::default(),
        extra_data: vec![],
        state_root: Blake2bHash::default(),
        body_root: Blake2bHash::default(),
        history_root: Blake2bHash::default(),
    };

    let mut micro_header_1 = micro_header.clone();
    micro_header_1.block_number += 1;

    let mut micro_header_2 = micro_header_1.clone();
    micro_header_2.block_number += 1;

    let fork_proof_1 = ForkProof {
        header1: micro_header.clone(),
        header2: micro_header_1.clone(),
        justification1: Signature::default(),
        justification2: Signature::default(),
        prev_vrf_seed: VrfSeed::default(),
    };

    let mut fork_proof_2 = fork_proof_1.clone();
    fork_proof_2.header2 = micro_header_2;

    let mut fork_proof_3 = fork_proof_2.clone();
    fork_proof_3.header1 = micro_header_1;

    let micro_justification = MicroJustification::Micro(Signature::default());

    let mut fork_proofs = vec![fork_proof_1, fork_proof_2, fork_proof_3];
    let micro_body = MicroBody {
        fork_proofs: fork_proofs.clone(),
        transactions: [].to_vec(),
    };

    // Build a block with body with a the unsorted fork proofs
    micro_header.body_root = micro_body.hash();
    let block = Block::Micro(MicroBlock {
        header: micro_header.clone(),
        justification: Some(micro_justification.clone()),
        body: Some(micro_body.clone()),
    });

    // The body check should fail
    assert_eq!(block.verify(false), Err(BlockError::ForkProofsNotOrdered));

    // Sort fork proofs and re-build block
    fork_proofs.sort();
    let micro_body = MicroBody {
        fork_proofs: fork_proofs.clone(),
        transactions: [].to_vec(),
    };

    micro_header.body_root = micro_body.hash();
    let block = Block::Micro(MicroBlock {
        header: micro_header.clone(),
        justification: Some(micro_justification.clone()),
        body: Some(micro_body.clone()),
    });

    assert_eq!(block.verify(false), Ok(()));

    // Lets have a duplicate fork proof
    fork_proofs.push(fork_proofs.last().unwrap().clone());
    let micro_body = MicroBody {
        fork_proofs: fork_proofs.clone(),
        transactions: [].to_vec(),
    };

    micro_header.body_root = micro_body.hash();
    let block = Block::Micro(MicroBlock {
        header: micro_header.clone(),
        justification: Some(micro_justification.clone()),
        body: Some(micro_body.clone()),
    });

    assert_eq!(block.verify(false), Err(BlockError::DuplicateForkProof));

    // Now modify the block height of the first header of the first fork proof
    let mut fork_proof = fork_proofs.pop().unwrap();
    fork_proof.header1.block_number = Policy::blocks_per_epoch();
    fork_proofs.push(fork_proof);
    fork_proofs.sort();

    let micro_body = MicroBody {
        fork_proofs: fork_proofs.clone(),
        transactions: [].to_vec(),
    };

    micro_header.body_root = micro_body.hash();
    let block = Block::Micro(MicroBlock {
        header: micro_header.clone(),
        justification: Some(micro_justification.clone()),
        body: Some(micro_body.clone()),
    });

    // The first fork proof should no longer be valid
    assert_eq!(block.verify(false), Err(BlockError::InvalidForkProof));
}

#[test]
fn test_verify_election_macro_body() {
    let mut macro_header = MacroHeader {
        version: Policy::VERSION,
        block_number: Policy::blocks_per_epoch(),
        round: 0,
        timestamp: 0,
        parent_hash: Blake2bHash::default(),
        parent_election_hash: Blake2bHash::default(),
        seed: VrfSeed::default(),
        extra_data: vec![0; 30],
        state_root: Blake2bHash::default(),
        body_root: Blake2bHash::default(),
        history_root: Blake2bHash::default(),
    };

    let mut macro_body = MacroBody {
        validators: None,
        pk_tree_root: None,
        lost_reward_set: BitSet::default(),
        disabled_set: BitSet::default(),
    };
    macro_header.body_root = macro_body.hash();

    // Build an election macro block
    let block = Block::Macro(MacroBlock {
        header: macro_header.clone(),
        justification: None,
        body: Some(macro_body.clone()),
    });

    // The validators check should fail
    assert_eq!(block.verify(false), Err(BlockError::InvalidValidators));

    // Fix the validators set
    macro_body.validators = Some(Validators::new(vec![]));
    macro_header.body_root = macro_body.hash();
    let block = Block::Macro(MacroBlock {
        header: macro_header.clone(),
        justification: None,
        body: Some(macro_body.clone()),
    });
    // The PK tree root check should fail
    assert_eq!(block.verify(false), Err(BlockError::InvalidPkTreeRoot));

    // Fix the PK tree root set
    macro_body.pk_tree_root = Some(vec![]);
    macro_header.body_root = macro_body.hash();
    let block = Block::Macro(MacroBlock {
        header: macro_header.clone(),
        justification: None,
        body: Some(macro_body.clone()),
    });

    // Verification would fail since validators are empty
    assert_eq!(block.verify(true), Err(BlockError::InvalidValidators));

    // Skipping the verification of the PK tree root should make the verify function to pass
    assert_eq!(block.verify(false), Ok(()));
}
