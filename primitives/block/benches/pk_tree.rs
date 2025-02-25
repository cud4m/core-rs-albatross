#[macro_use]
extern crate bencher;

use bencher::Bencher;
use nimiq_block::MacroBlock;
use nimiq_bls::lazy::LazyPublicKey;
use nimiq_bls::KeyPair as BlsKeyPair;
use nimiq_keys::{Address, KeyPair as SchnorrKeyPair, SecureGenerate};
use nimiq_primitives::policy::Policy;
use nimiq_primitives::slots::{Validators, ValidatorsBuilder};
use nimiq_test_utils::zkp_test_data::get_base_seed;

// Benchmarks for the pk tree root computation.
// Calculating the pk tree root for a large number of validators is an expensive task.

/// Generate validators for the benchmarks.
fn generate_uncompressed_validators() -> Validators {
    let mut rng = get_base_seed();
    let mut validators = ValidatorsBuilder::new();

    for _ in 0..Policy::SLOTS {
        let keypair = SchnorrKeyPair::generate(&mut rng);
        let address = Address::from(&keypair.public);
        let bls_keypair = BlsKeyPair::generate(&mut rng);
        validators.push(address, bls_keypair.public_key, keypair.public);
    }

    validators.build()
}

/// We compute the pk tree root in the worst-case scenario:
/// Every slot is taken by a different validator and all public keys are compressed,
/// i.e., they need uncompressing.
fn fully_compressed(bench: &mut Bencher) {
    let validators = generate_uncompressed_validators();
    // Forget uncompressed keys.
    let compressed_validators = validators
        .iter()
        .map(|v| {
            let mut new_v = v.clone();
            new_v.voting_key = LazyPublicKey::from_compressed(v.voting_key.compressed());
            new_v
        })
        .collect();
    let validators = Validators::new(compressed_validators);

    bench.iter(|| MacroBlock::pk_tree_root(&validators))
}

/// We compute the pk tree root in the following scenario:
/// Every slot is taken by a different validator and all public keys are already uncompressed,
/// i.e., they do *not* need uncompressing.
fn fully_uncompressed(bench: &mut Bencher) {
    let validators = generate_uncompressed_validators();

    bench.iter(|| MacroBlock::pk_tree_root(&validators))
}

benchmark_group!(benches, fully_compressed, fully_uncompressed);
benchmark_main!(benches);
