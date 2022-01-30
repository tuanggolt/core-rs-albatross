use ark_crypto_primitives::crh::poseidon::constraints::{CRHGadget, CRHParametersVar};
use ark_crypto_primitives::CRHSchemeGadget;
use ark_groth16::constraints::VerifyingKeyVar;
use ark_mnt4_753::Fr as MNT4Fr;
use ark_mnt6_753::constraints::PairingVar;
use ark_mnt6_753::MNT6_753;
use ark_r1cs_std::prelude::Boolean;
use ark_r1cs_std::{R1CSVar, ToBitsGadget, ToConstraintFieldGadget};
use ark_relations::r1cs::SynthesisError;

/// This gadget is meant to calculate a commitment in-circuit for a verifying key of a SNARK in the
/// MNT6-753 curve. This means we can open this commitment inside of a circuit in the MNT4-753 curve
/// and we can use it to verify a SNARK proof inside that circuit.
/// We calculate it by first separating the verifying key into field elements and feeding them to
/// the Poseidon hash function, then we serialize the output and convert it to bits. This provides
/// an efficient way of compressing the state and representing it across different curves.
pub struct VKCommitmentGadget;

impl VKCommitmentGadget {
    /// Calculates the verifying key commitment.
    pub fn evaluate(
        vk: &VerifyingKeyVar<MNT6_753, PairingVar>,
        poseidon_params: &CRHParametersVar<MNT4Fr>,
    ) -> Result<Vec<Boolean<MNT4Fr>>, SynthesisError> {
        // Initialize the field elements vector.
        let mut elements = vec![];

        // Separate the verifying key into field elements.
        // Alpha G1
        elements.append(&mut vk.alpha_g1.to_constraint_field()?);

        // // Beta G2
        elements.append(&mut vk.beta_g2.to_constraint_field()?);

        // Gamma G2
        elements.append(&mut vk.gamma_g2.to_constraint_field()?);

        // Delta G2
        elements.append(&mut vk.delta_g2.to_constraint_field()?);

        // Gamma ABC G1
        for i in 0..vk.gamma_abc_g1.len() {
            elements.append(&mut vk.gamma_abc_g1[i].to_constraint_field()?);
        }

        // Calculate the hash.
        let hash = CRHGadget::<MNT4Fr>::evaluate(poseidon_params, &elements)?;

        // Serialize the hash.
        let mut hash_bits = hash.to_bits_be()?;

        // We discard the first bit since public inputs in our circuits can only have 752 bits.
        hash_bits.remove(0);

        Ok(hash_bits)
    }
}

#[cfg(test)]
mod tests {
    use ark_crypto_primitives::crh::poseidon::constraints::CRHParametersVar;
    use ark_ec::ProjectiveCurve;
    use ark_groth16::constraints::VerifyingKeyVar;
    use ark_groth16::VerifyingKey;
    use ark_mnt4_753::Fr as MNT4Fr;
    use ark_mnt6_753::constraints::PairingVar;
    use ark_mnt6_753::MNT6_753;
    use ark_mnt6_753::{G1Projective, G2Projective};
    use ark_r1cs_std::prelude::AllocVar;
    use ark_r1cs_std::R1CSVar;
    use ark_relations::r1cs::ConstraintSystem;
    use ark_std::{test_rng, UniformRand};

    use nimiq_bls::utils::bytes_to_bits;
    use nimiq_nano_primitives::mnt6::poseidon_mnt6_t9_parameters;
    use nimiq_nano_primitives::vk_commitment;

    use crate::gadgets::mnt4::VKCommitmentGadget;

    #[test]
    fn vk_commitment_test() {
        // Initialize the constraint system.
        let cs = ConstraintSystem::<MNT4Fr>::new_ref();

        // Create the Poseidon parameters.
        let poseidon_params = poseidon_mnt6_t9_parameters();

        // Create random number generator.
        let rng = &mut test_rng();

        // Create verifying key.
        let mut vk = VerifyingKey::<MNT6_753>::default();
        vk.alpha_g1 = G1Projective::rand(rng).into_affine();
        vk.beta_g2 = G2Projective::rand(rng).into_affine();
        vk.gamma_g2 = G2Projective::rand(rng).into_affine();
        vk.delta_g2 = G2Projective::rand(rng).into_affine();
        vk.gamma_abc_g1 = vec![
            G1Projective::rand(rng).into_affine(),
            G1Projective::rand(rng).into_affine(),
        ];

        // Evaluate vk commitment using the primitive version.
        let primitive_comm = bytes_to_bits(&vk_commitment(&vk, &poseidon_params));

        // Allocate the verifying key in the circuit.
        let vk_var = VerifyingKeyVar::<_, PairingVar>::new_witness(cs.clone(), || Ok(vk)).unwrap();

        // Allocate the Poseidon parameters in the circuit.
        let poseidon_var =
            CRHParametersVar::<MNT4Fr>::new_witness(cs.clone(), || Ok(poseidon_params)).unwrap();

        // Evaluate vk commitment using the gadget version.
        let gadget_comm = VKCommitmentGadget::evaluate(&vk_var, &poseidon_var).unwrap();

        // Compare the two versions bit by bit.
        assert_eq!(primitive_comm.len(), gadget_comm.len());
        for i in 0..gadget_comm.len() {
            assert_eq!(primitive_comm[i], gadget_comm[i].value().unwrap());
        }
    }
}
