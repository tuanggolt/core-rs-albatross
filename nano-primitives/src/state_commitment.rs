use ark_crypto_primitives::crh::poseidon::{TwoToOneCRH, CRH};
use ark_crypto_primitives::crh::TwoToOneCRHScheme;
use ark_crypto_primitives::CRHScheme;
use ark_ff::PrimeField;
use ark_mnt6_753::{Fq, G1Projective};
use ark_sponge::poseidon::PoseidonParameters;

use nimiq_bls::pedersen::{pedersen_generators, pedersen_hash};
use nimiq_bls::utils::{big_int_from_bytes_be, bytes_to_bits};

use crate::{pk_tree_construct, serialize_fq_mnt6, serialize_g1_mnt6};

/// This gadget is meant to calculate the "state commitment" off-circuit, which is simply a commitment,
/// for a given block, of the block number concatenated with the header hash concatenated with the
/// root of a Merkle tree over the public keys.
/// We calculate it by first creating a Merkle tree from the public keys. Then we serialize the
/// block number, the header hash and the Merkle tree root and feed it to the Poseidon hash function.
/// Lastly we serialize the output and convert it to bytes. This provides an efficient way of
/// compressing the state and representing it across different curves.
/// Note that we discard the first byte of the resulting since the original commitment
/// gadget only returns 752 bits.
pub fn state_commitment(
    block_number: u32,
    header_hash: [u8; 32],
    public_keys: Vec<G1Projective>,
    poseidon_params: &PoseidonParameters<Fq>,
) -> Vec<u8> {
    // Initialize the vector to create the first field element. We'll pad it with 60 bytes.
    let mut bytes: Vec<u8> = vec![0u8; 60];

    // Serialize the block number and header hash.
    bytes.extend_from_slice(&block_number.to_be_bytes());
    bytes.extend_from_slice(&header_hash);
    debug_assert_eq!(bytes.len(), 96);

    // Create the first field element.
    let elem_1 = Fq::from_repr(big_int_from_bytes_be(&mut &bytes[..])).unwrap();

    // Construct the Merkle tree over the public keys.
    let root_bytes = pk_tree_construct(public_keys);
    debug_assert_eq!(root_bytes.len(), 95);

    // Create the second field element.
    let elem_2 = Fq::from_repr(big_int_from_bytes_be(&mut &root_bytes[..])).unwrap();

    // Calculate the hash.
    let hash = TwoToOneCRH::<Fq>::evaluate(poseidon_params, elem_1, elem_2).unwrap();

    // Serialize the hash.
    let bytes = serialize_fq_mnt6(&hash);

    bytes[1..].to_vec()
}
