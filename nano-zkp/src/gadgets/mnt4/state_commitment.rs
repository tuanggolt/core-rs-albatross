use ark_crypto_primitives::crh::poseidon::constraints::{
    CRHGadget, CRHParametersVar, TwoToOneCRHGadget,
};
use ark_crypto_primitives::crh::TwoToOneCRHSchemeGadget;
use ark_mnt4_753::Fr as MNT4Fr;
use ark_mnt6_753::constraints::G1Var;
use ark_r1cs_std::bits::boolean::Boolean;
use ark_r1cs_std::prelude::UInt32;
use ark_r1cs_std::ToBitsGadget;
use ark_relations::r1cs::{ConstraintSystemRef, SynthesisError};

/// This gadget is meant to calculate the "state commitment" in-circuit, which is simply a commitment,
/// for a given block, of the block number concatenated with the root of a Merkle tree over the public
/// keys. We don't calculate the Merkle tree from the public keys. We just serialize the block number
/// and the Merkle tree root, feed it to the Poseidon hash function and serialize the output. This
/// provides an efficient way of compressing the state and representing it across different curves.
pub struct StateCommitmentGadget;

impl StateCommitmentGadget {
    /// Calculates the state commitment.
    pub fn evaluate(
        block_number: &UInt32<MNT4Fr>,
        header_hash: &[Boolean<MNT4Fr>],
        pk_tree_root: &[Boolean<MNT4Fr>],
        poseidon_params: &CRHParametersVar<MNT4Fr>,
    ) -> Result<Vec<Boolean<MNT4Fr>>, SynthesisError> {
        // Initialize Boolean vector for the first field element.
        let mut bits = vec![];

        // Reverse and append the header hash.
        let mut header_hash_le = header_hash.to_vec();
        header_hash_le.reverse();
        bits.append(&mut header_hash_le);

        // Append the block number.
        bits.extend(block_number.to_bits_le());

        // Create the first field element.
        let elem_1 = Boolean::le_bits_to_fp_var(&bits)?;

        // Create the second field element from the public key tree root.
        let elem_2 = Boolean::le_bits_to_fp_var(&pk_tree_root)?;

        // Calculate the hash.
        let hash = TwoToOneCRHGadget::<MNT4Fr>::evaluate(poseidon_params, &elem_1, &elem_2)?;

        // Serialize the hash and return it.
        hash.to_bits_be()
    }
}

#[cfg(test)]
mod tests {
    use ark_mnt4_753::Fr as MNT4Fr;
    use ark_mnt6_753::constraints::G1Var;
    use ark_mnt6_753::G1Projective;
    use ark_r1cs_std::prelude::{AllocVar, Boolean, UInt32};
    use ark_r1cs_std::R1CSVar;
    use ark_relations::r1cs::ConstraintSystem;
    use ark_std::{test_rng, UniformRand};
    use rand::RngCore;

    use nimiq_bls::pedersen::pedersen_generators;
    use nimiq_bls::utils::bytes_to_bits;
    use nimiq_nano_primitives::mnt6::poseidon_mnt6_t3_parameters;
    use nimiq_nano_primitives::{pk_tree_construct, state_commitment};
    use nimiq_primitives::policy::SLOTS;

    use super::*;

    #[test]
    fn state_commitment_works() {
        // Initialize the constraint system.
        let cs = ConstraintSystem::<MNT4Fr>::new_ref();

        // Create the Poseidon parameters.
        let poseidon_params = poseidon_mnt6_t3_parameters();

        // Create random number generator.
        let rng = &mut test_rng();

        // Create random keys.
        let g2_point = G1Projective::rand(rng);
        let public_keys = vec![g2_point; SLOTS as usize];

        // Create random block number.
        let block_number = u32::rand(rng);

        // Create random header hash.
        let mut header_hash = [0u8; 32];
        rng.fill_bytes(&mut header_hash);

        // Evaluate state commitment using the primitive version.
        let primitive_comm = bytes_to_bits(&state_commitment(
            block_number,
            header_hash,
            public_keys.clone(),
            &poseidon_params,
        ));

        // Convert the header hash to bits.
        let header_hash_bits = bytes_to_bits(&header_hash);

        // Construct the Merkle tree over the public keys.
        let pk_tree_root = pk_tree_construct(public_keys);
        let pk_tree_root_bits = bytes_to_bits(&pk_tree_root);

        // Allocate the block number in the circuit.
        let block_number_var = UInt32::new_witness(cs.clone(), || Ok(block_number)).unwrap();

        // Allocate the header hash in the circuit.
        let header_hash_var =
            Vec::<Boolean<MNT4Fr>>::new_witness(cs.clone(), || Ok(header_hash_bits)).unwrap();

        // Allocate the public key tree root in the circuit.
        let pk_tree_root_var =
            Vec::<Boolean<MNT4Fr>>::new_witness(cs.clone(), || Ok(pk_tree_root_bits)).unwrap();

        // Allocate the Poseidon parameters in the circuit.
        let poseidon_var =
            CRHParametersVar::<MNT4Fr>::new_witness(cs.clone(), || Ok(poseidon_params)).unwrap();

        // Evaluate state commitment using the gadget version.
        let gadget_comm = StateCommitmentGadget::evaluate(
            &block_number_var,
            &header_hash_var,
            &pk_tree_root_var,
            &poseidon_var,
        )
        .unwrap();

        // Compare the two versions bit by bit. The first 7 bits of the primitive version are padding.
        assert_eq!(primitive_comm.len(), gadget_comm.len() + 7);
        for i in 0..gadget_comm.len() {
            assert_eq!(primitive_comm[i + 7], gadget_comm[i].value().unwrap());
        }
    }
}
