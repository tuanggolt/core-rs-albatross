use ark_crypto_primitives::crh::poseidon::CRH;
use ark_crypto_primitives::CRHScheme;
use ark_ec::AffineCurve;
use ark_ff::ToConstraintField;
use ark_groth16::VerifyingKey;
use ark_mnt6_753::{Fq, MNT6_753};
use ark_sponge::poseidon::PoseidonParameters;

use crate::serialize_fq_mnt6;

/// This function is meant to calculate a commitment off-circuit for a verifying key of a SNARK in the
/// MNT6-753 curve. This means we can open this commitment inside of a circuit in the MNT4-753 curve
/// and we can use it to verify a SNARK proof inside that circuit.
/// We calculate it by first separating the verifying key into field elements and feeding them to
/// the Poseidon hash function, then we serialize the output and convert it to bits. This provides
/// an efficient way of compressing the state and representing it across different curves.
/// Note that the first 7 bits of the resulting vector will be padding since the original commitment
/// gadget only returns 753 bits.
pub fn vk_commitment(
    vk: &VerifyingKey<MNT6_753>,
    poseidon_params: &PoseidonParameters<Fq>,
) -> Vec<u8> {
    // Initialize the field elements vector.
    let mut elements = vec![];

    // Separate the verifying key into field elements.
    // Alpha G1
    elements.append(&mut vk.alpha_g1.to_field_elements().unwrap());

    // Beta G2
    elements.append(&mut vk.beta_g2.to_field_elements().unwrap());

    // Gamma G2
    elements.append(&mut vk.gamma_g2.to_field_elements().unwrap());

    // Delta G2
    elements.append(&mut vk.delta_g2.to_field_elements().unwrap());

    // Gamma ABC G1
    for i in 0..vk.gamma_abc_g1.len() {
        elements.append(&mut vk.gamma_abc_g1[i].to_field_elements().unwrap());
    }

    // Calculate the hash.
    let hash = CRH::<Fq>::evaluate(poseidon_params, elements).unwrap();

    // Serialize the hash.
    let bytes = serialize_fq_mnt6(&hash);

    Vec::from(bytes.as_ref())
}
