/// Generic implementation of a Handel protocol for use with Nimiq's weighted voting between validators.


use std::sync::Arc;
use std::io::Error as IoError;
use std::io::ErrorKind;
use std::collections::HashMap;
use std::fmt;

use parking_lot::RwLock;

use primitives::validators::Validators;
use primitives::policy::TWO_THIRD_SLOTS;
use block_albatross::signed;
use messages::Message;
use bls::bls12_381::PublicKey;

use handel::protocol::Protocol;
use handel::multisig::{IndividualSignature, Signature};
use handel::identity::{IdentityRegistry, WeightRegistry};
use handel::verifier::MultithreadedVerifier;
use handel::timeout::LinearTimeout;
use handel::config::Config;
use handel::store::ReplaceStore;
use handel::partitioner::BinomialPartitioner;
use handel::evaluator::WeightedVote;
use handel::update::{LevelUpdate, LevelUpdateMessage};
use handel::aggregation::Aggregation;
use handel::store::SignatureStore;
use handel::sender::Sender;

use crate::validator_agent::ValidatorAgent;




/// The evaluator used for voting
/// TODO: The one for commit, needs to consider the prepare votes as well.
pub type VotingEvaluator = WeightedVote<ReplaceStore<BinomialPartitioner>, ValidatorRegistry, BinomialPartitioner>;



pub trait Tag: signed::Message {
    // TODO: This should not be implemented by the tag, right?
    fn create_level_update_message(&self, update: LevelUpdate) -> Message;
}



/// Implementation for sender using a mapping from validator ID to `Peer`.
pub struct VotingSender<T: Tag> {
    pub tag: T,
    pub peers: Arc<HashMap<usize, Arc<ValidatorAgent>>>,
}

impl<T: Tag> VotingSender<T> {
    pub fn new(tag: T, peers: Arc<HashMap<usize, Arc<ValidatorAgent>>>) -> Self {
        Self {
            tag,
            peers
        }
    }
}

impl<T: Tag> Sender for VotingSender<T> {
    type Error = IoError;

    fn send_to(&self, peer_id: usize, update: LevelUpdate) -> Result<(), IoError> {
        if let Some(agent) = self.peers.get(&peer_id) {
            let update_message = self.tag.create_level_update_message(update);
            agent.peer.channel.send(update_message)
                .map_err(|e| IoError::new(ErrorKind::Other, e))
        }
        else {
            //warn!("No peer for validator ID {}", to);
            Ok(())
        }
    }
}



/// Implementation for handel registry using a `Validators` list.
pub struct ValidatorRegistry {
    validators: Validators,
}

impl IdentityRegistry for ValidatorRegistry {
    fn public_key(&self, id: usize) -> Option<PublicKey> {
        self.validators.get(id).and_then(|validator| validator.1.uncompressed())
    }
}

impl WeightRegistry for ValidatorRegistry {
    fn weight(&self, id: usize) -> Option<usize> {
        self.validators.get(id).map(|validator| validator.0 as usize)
    }
}



/// The generic protocol implementation for validator voting
pub struct VotingProtocol<T: Tag> {
    pub tag: T,

    /// The validator ID
    pub node_id: usize,

    registry: Arc<ValidatorRegistry>,

    // TODO: This should not be part of the protocol. Except that it holds the hash which depends
    // on the *specific* item being hashed (including prefix)
    verifier: Arc<MultithreadedVerifier<ValidatorRegistry>>,

    partitioner: Arc<BinomialPartitioner>,
    store: Arc<RwLock<ReplaceStore<BinomialPartitioner>>>,

    /// The evaluator being used. This either just counts votes
    evaluator: Arc<VotingEvaluator>,

    sender: Arc<VotingSender<T>>,
}

impl<T: Tag> VotingProtocol<T> {
    pub fn new(tag: T, node_id: usize, validators: Validators, config: &Config, peers: Arc<HashMap<usize, Arc<ValidatorAgent>>>) -> Self {
        let num_validators = validators.num_groups();
        trace!("num_validators = {}", num_validators);
        trace!("validator_id = {}", node_id);
        for (&peer_id, agent) in peers.iter() {
            trace!("peer {}: {}", peer_id, agent.peer.peer_address());
        }

        let registry = Arc::new(ValidatorRegistry {
            validators,
        });
        let verifier = Arc::new(MultithreadedVerifier::new(
            tag.hash_with_prefix(),
            Arc::clone(&registry),
            None,
        ));
        let timeouts = Arc::new(LinearTimeout::new(config.timeout));
        let partitioner = Arc::new(BinomialPartitioner::new(
            node_id,
            num_validators,
        ));
        let store = Arc::new(RwLock::new(ReplaceStore::new(Arc::clone(&partitioner))));
        let evaluator = Arc::new(WeightedVote::new(
            Arc::clone(&store),
            Arc::clone(&registry),
            Arc::clone(&partitioner),
            TWO_THIRD_SLOTS as usize,
        ));
        let sender = Arc::new(VotingSender {
            tag: tag.clone(),
            peers,
        });

        Self {
            tag,
            registry,
            verifier,
            partitioner,
            store,
            evaluator,
            node_id,
            sender,
        }
    }

    pub fn votes(&self) -> usize {
        let store = self.store.read();
        store.best(store.best_level())
            .map(|multisig| {
                self.registry.signature_weight(&Signature::Multi(multisig.clone()))
                    .unwrap_or_else(|| panic!("Unknown signers in signature: {:?}", multisig))
            })
            .unwrap_or(0)
    }
}

impl<T: Tag> fmt::Debug for VotingProtocol<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "VotingProtocol {{ node_id: {}, {:?} }}", self.node_id, self.tag)
    }
}

impl<T: Tag> Protocol for VotingProtocol<T> {
    type Registry = ValidatorRegistry;
    type Verifier = MultithreadedVerifier<ValidatorRegistry>;
    type Store = ReplaceStore<BinomialPartitioner>;
    type Evaluator = VotingEvaluator;
    type Partitioner = BinomialPartitioner;
    type Sender = VotingSender<T>;

    fn registry(&self) -> Arc<Self::Registry> {
        Arc::clone(&self.registry)
    }

    fn verifier(&self) -> Arc<Self::Verifier> {
        Arc::clone(&self.verifier)
    }

    fn store(&self) -> Arc<RwLock<Self::Store>> {
        Arc::clone(&self.store)
    }

    fn evaluator(&self) -> Arc<Self::Evaluator> {
        Arc::clone(&self.evaluator)
    }

    fn partitioner(&self) -> Arc<Self::Partitioner> {
        Arc::clone(&self.partitioner)
    }

    fn sender(&self) -> Arc<Self::Sender> {
        Arc::clone(&self.sender)
    }

    fn node_id(&self) -> usize {
        self.node_id
    }
}


/// Wrapper to make life easier ;)
pub struct VoteAggregation<T: Tag> {
    pub inner: Arc<Aggregation<VotingProtocol<T>>>
}

impl<T: Tag> VoteAggregation<T> {
    pub fn new(tag: T, node_id: usize, validators: Validators, peers: Arc<HashMap<usize, Arc<ValidatorAgent>>>, config: Option<Config>) -> Self {
        let config = config.unwrap_or_default();
        let protocol = VotingProtocol::new(tag, node_id, validators, &config, peers);
        let aggregation = Aggregation::new(protocol, config);
        Self { inner: aggregation }
    }

    pub fn push_contribution(&self, contribution: signed::SignedMessage<T>) {
        // deconstruct signed view change
        let signed::SignedMessage {
            signature,
            message: tag,
            signer_idx: node_id,
        } = contribution;
        let node_id = node_id as usize;

        // panic if the contribution doesn't belong to this aggregation
        if self.inner.protocol.tag != tag {
            panic!("Submitting contribution for {:?}, but aggregation is for {:?}", tag, self.tag());
        }

        // panic if the contribution is from a different node
        if self.inner.protocol.node_id != node_id {
            panic!("Submitting contribution for validator {}, but aggregation is running as validator {}", node_id, self.node_id());
        }

        self.inner.push_contribution(IndividualSignature::new(signature, node_id));
    }

    pub fn push_update(&self, level_update: LevelUpdateMessage<T>) {
        if level_update.tag != *self.tag() {
            panic!("Submitting level update for {:?}, but aggregation is for {:?}");
        }
        self.inner.push_update(level_update.update);
    }

    pub fn votes(&self) -> usize {
        self.inner.protocol.votes()
    }

    pub fn node_id(&self) -> usize {
        self.inner.protocol.node_id
    }

    pub fn tag(&self) -> &T {
        &self.inner.protocol.tag
    }
}
