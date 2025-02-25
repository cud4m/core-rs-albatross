use std::path::{Path, PathBuf};
use std::sync::Arc;

use beserial::Deserialize;
use futures::StreamExt;

use nimiq_block_production::BlockProducer;
use nimiq_blockchain::{Blockchain, BlockchainConfig};
use nimiq_blockchain_proxy::BlockchainProxy;
use nimiq_database::volatile::VolatileEnvironment;
use nimiq_nano_zkp::NanoZKP;
use nimiq_network_interface::network::Network;
use nimiq_network_mock::MockHub;
use nimiq_primitives::networks::NetworkId;
use nimiq_primitives::policy::Policy;
use nimiq_test_log::test;
use nimiq_test_utils::blockchain::{signing_key, voting_key};
use nimiq_test_utils::blockchain_with_rng::produce_macro_blocks_with_rng;
use nimiq_test_utils::zkp_test_data::{get_base_seed, zkp_test_exe};
use nimiq_test_utils::zkp_test_data::{KEYS_PATH, ZKPROOF_SERIALIZED_IN_HEX};

use nimiq_zkp_component::proof_utils::{validate_proof, ProofStore};
use nimiq_zkp_component::types::ZKProof;
use nimiq_zkp_component::zkp_requests::ZKPRequests;
use nimiq_zkp_component::ZKPComponent;
use parking_lot::RwLock;

use nimiq_utils::time::OffsetTime;

fn blockchain() -> Arc<RwLock<Blockchain>> {
    let time = Arc::new(OffsetTime::new());
    let env = VolatileEnvironment::new(10).unwrap();
    Arc::new(RwLock::new(
        Blockchain::new(
            env,
            BlockchainConfig::default(),
            NetworkId::UnitAlbatross,
            time,
        )
        .unwrap(),
    ))
}

#[test(tokio::test)]
async fn peers_dont_reply_with_outdated_proof() {
    NanoZKP::setup(get_base_seed(), Path::new(KEYS_PATH), false).unwrap();
    let blockchain = blockchain();
    let mut hub = MockHub::new();
    let network = Arc::new(hub.new_network());
    let network2 = Arc::new(hub.new_network());
    let network3 = Arc::new(hub.new_network());
    network.dial_address(network3.address()).await.unwrap();
    network.dial_address(network2.address()).await.unwrap();

    let _zkp_prover2 = ZKPComponent::new(
        BlockchainProxy::from(&blockchain),
        Arc::clone(&network2),
        false,
        Some(zkp_test_exe()),
        VolatileEnvironment::new(10).unwrap(),
        PathBuf::from(KEYS_PATH),
    )
    .await;

    let _zkp_prover3 = ZKPComponent::new(
        BlockchainProxy::from(&blockchain),
        Arc::clone(&network3),
        false,
        Some(zkp_test_exe()),
        VolatileEnvironment::new(10).unwrap(),
        PathBuf::from(KEYS_PATH),
    )
    .await;

    let mut zkp_requests = ZKPRequests::new(Arc::clone(&network));

    // Trigger zkp requests
    zkp_requests.request_zkps(network.get_peers(), 0, false);

    for _ in 0..2 {
        assert!(
            zkp_requests.next().await.is_none(),
            "Peer sent a proof when it should have abstained because of having an outdated proof"
        );
    }
}

#[test(tokio::test)]
async fn peers_reply_with_valid_proof() {
    NanoZKP::setup(get_base_seed(), Path::new(KEYS_PATH), false).unwrap();
    let blockchain2 = blockchain();
    let blockchain3 = blockchain();
    let mut hub = MockHub::new();
    let network = Arc::new(hub.new_network());
    let network2 = Arc::new(hub.new_network());
    let network3 = Arc::new(hub.new_network());
    network.dial_address(network3.address()).await.unwrap();
    network.dial_address(network2.address()).await.unwrap();

    let env2 = VolatileEnvironment::new(10).unwrap();
    let env3 = VolatileEnvironment::new(10).unwrap();
    let store2 = ProofStore::new(env2.clone());
    let store3 = ProofStore::new(env3.clone());
    let producer = BlockProducer::new(signing_key(), voting_key());
    produce_macro_blocks_with_rng(
        &producer,
        &blockchain2,
        Policy::batches_per_epoch() as usize,
        &mut get_base_seed(),
    );
    produce_macro_blocks_with_rng(
        &producer,
        &blockchain3,
        Policy::batches_per_epoch() as usize,
        &mut get_base_seed(),
    );

    // Seta valid proof into the 2 components.
    let new_proof =
        &ZKProof::deserialize_from_vec(&hex::decode(ZKPROOF_SERIALIZED_IN_HEX).unwrap()).unwrap();
    log::info!("setting proof");
    store2.set_zkp(&new_proof);
    store3.set_zkp(&new_proof);

    log::info!("launching zkps");
    let _zkp_prover2 = ZKPComponent::new(
        BlockchainProxy::from(&blockchain2),
        Arc::clone(&network2),
        false,
        Some(zkp_test_exe()),
        env2,
        PathBuf::from(KEYS_PATH),
    )
    .await;
    let _zkp_prover3 = ZKPComponent::new(
        BlockchainProxy::from(&blockchain3),
        Arc::clone(&network3),
        false,
        Some(zkp_test_exe()),
        env3,
        PathBuf::from(KEYS_PATH),
    )
    .await;

    let mut zkp_requests = ZKPRequests::new(Arc::clone(&network));

    // Trigger zkp requests from the first component.
    zkp_requests.request_zkps(network.get_peers(), 0, false);

    for _ in 0..2 {
        let proof_data = zkp_requests.next().await.unwrap();
        assert!(
            proof_data.election_block.is_none(),
            "Peers should not send an election block"
        );
        assert!(
            validate_proof(
                &BlockchainProxy::from(&blockchain2),
                &proof_data.proof,
                None,
                Path::new(KEYS_PATH)
            ),
            "Peer should sent a new proof valid proof"
        );
    }
}

#[test(tokio::test)]
async fn peers_reply_with_valid_proof_and_election_block() {
    NanoZKP::setup(get_base_seed(), Path::new(KEYS_PATH), false).unwrap();
    let blockchain2 = blockchain();
    let blockchain3 = blockchain();
    let mut hub = MockHub::new();
    let network = Arc::new(hub.new_network());
    let network2 = Arc::new(hub.new_network());
    let network3 = Arc::new(hub.new_network());
    network.dial_address(network3.address()).await.unwrap();
    network.dial_address(network2.address()).await.unwrap();

    let env2 = VolatileEnvironment::new(10).unwrap();
    let env3 = VolatileEnvironment::new(10).unwrap();
    let store2 = ProofStore::new(env2.clone());
    let store3 = ProofStore::new(env3.clone());
    let producer = BlockProducer::new(signing_key(), voting_key());
    produce_macro_blocks_with_rng(
        &producer,
        &blockchain2,
        Policy::batches_per_epoch() as usize,
        &mut get_base_seed(),
    );
    produce_macro_blocks_with_rng(
        &producer,
        &blockchain3,
        Policy::batches_per_epoch() as usize,
        &mut get_base_seed(),
    );

    // Seta valid proof into the 2 components.
    let new_proof =
        &ZKProof::deserialize_from_vec(&hex::decode(ZKPROOF_SERIALIZED_IN_HEX).unwrap()).unwrap();
    log::info!("setting proof");
    store2.set_zkp(&new_proof);
    store3.set_zkp(&new_proof);

    log::info!("launching zkps");
    let _zkp_prover2 = ZKPComponent::new(
        BlockchainProxy::from(&blockchain2),
        Arc::clone(&network2),
        false,
        Some(zkp_test_exe()),
        env2,
        PathBuf::from(KEYS_PATH),
    )
    .await;
    let _zkp_prover3 = ZKPComponent::new(
        BlockchainProxy::from(&blockchain3),
        Arc::clone(&network3),
        false,
        Some(zkp_test_exe()),
        env3,
        PathBuf::from(KEYS_PATH),
    )
    .await;

    let mut zkp_requests = ZKPRequests::new(Arc::clone(&network));

    // Trigger zkp requests from the first component.
    zkp_requests.request_zkps(network.get_peers(), 0, true);

    for _ in 0..2 {
        let proof_data = zkp_requests.next().await.unwrap();
        assert!(
            proof_data.election_block.is_some(),
            "Peers should send an election block"
        );
        assert!(
            validate_proof(
                &BlockchainProxy::from(&blockchain2),
                &proof_data.proof,
                proof_data.election_block,
                Path::new(KEYS_PATH)
            ),
            "Peer should sent a new proof valid proof"
        );
    }
}
