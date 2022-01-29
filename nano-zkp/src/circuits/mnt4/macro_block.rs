use ark_crypto_primitives::crh::poseidon::constraints::{CRHGadget, CRHParametersVar};
use ark_crypto_primitives::snark::BooleanInputVar;
use ark_crypto_primitives::{CRHSchemeGadget, SNARKGadget};
use ark_groth16::constraints::{Groth16VerifierGadget, ProofVar, VerifyingKeyVar};
use ark_groth16::{Proof, VerifyingKey};
use ark_mnt4_753::Fr as MNT4Fr;
use ark_mnt6_753::constraints::{FqVar, G1Var, PairingVar};
use ark_mnt6_753::{Fq, G1Projective, MNT6_753};
use ark_r1cs_std::prelude::{
    AllocVar, Boolean, CondSelectGadget, CurveVar, EqGadget, FieldVar, ToBitsGadget, UInt32,
};
use ark_r1cs_std::ToConstraintFieldGadget;
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};
use nimiq_bls::pedersen::pedersen_generators;
use nimiq_nano_primitives::mnt6::{poseidon_mnt6_t3_parameters, poseidon_mnt6_t9_parameters};
use nimiq_nano_primitives::MacroBlock;
use nimiq_primitives::policy::EPOCH_LENGTH;

use crate::gadgets::mnt4::{MacroBlockGadget, SerializeGadget, StateCommitmentGadget};
use crate::utils::unpack_inputs;

/// This is the macro block circuit. It takes as inputs an initial state commitment and final state commitment
/// and it produces a proof that there exists a valid macro block that transforms the initial state
/// into the final state.
/// Since the state is composed only of the block number and the public keys of the current validator
/// list, updating the state is just incrementing the block number and substituting the previous
/// public keys with the public keys of the new validator list.
#[derive(Clone)]
pub struct MacroBlockCircuit {
    // Witnesses (private)
    initial_pks: Vec<G1Projective>,
    initial_header_hash: Vec<bool>,
    block: MacroBlock,

    // Inputs (public)
    // Our inputs are always vectors of booleans (semantically), so that they are consistent across
    // the different elliptic curves that we use. However, for compactness, we represent them as
    // field elements. Both of the curves that we use have a modulus of 753 bits and a capacity
    // of 752 bits. So, the first 752 bits (in little-endian) of each field element is data, and the
    // last bit is always set to zero.
    initial_state_commitment: Fq,
    final_state_commitment: Fq,
}

impl MacroBlockCircuit {
    pub fn new(
        initial_pks: Vec<G1Projective>,
        initial_header_hash: Vec<bool>,
        block: MacroBlock,
        initial_state_commitment: Fq,
        final_state_commitment: Fq,
    ) -> Self {
        Self {
            initial_pks,
            initial_header_hash,
            block,
            initial_state_commitment,
            final_state_commitment,
        }
    }
}

impl ConstraintSynthesizer<MNT4Fr> for MacroBlockCircuit {
    /// This function generates the constraints for the circuit.
    fn generate_constraints(self, cs: ConstraintSystemRef<MNT4Fr>) -> Result<(), SynthesisError> {
        // -------------------------- Allocate variables ----------------------------------

        // Allocate all the constants.
        let epoch_length_var = UInt32::<MNT4Fr>::new_constant(cs.clone(), EPOCH_LENGTH)?;

        let poseidon_params_2_var = CRHParametersVar::<MNT4Fr>::new_witness(cs.clone(), || {
            Ok(poseidon_mnt6_t3_parameters())
        })
        .unwrap();

        let poseidon_params_8_var = CRHParametersVar::<MNT4Fr>::new_witness(cs.clone(), || {
            Ok(poseidon_mnt6_t9_parameters())
        })
        .unwrap();

        // Allocate all the witnesses.
        let initial_pks_var = Vec::<G1Var>::new_witness(cs.clone(), || Ok(&self.initial_pks[..]))?;

        let initial_header_hash_var =
            Vec::<Boolean<MNT4Fr>>::new_witness(cs.clone(), || Ok(&self.initial_header_hash[..]))?;

        let block_var = MacroBlockGadget::new_witness(cs.clone(), || Ok(&self.block))?;

        let initial_block_number_var =
            UInt32::new_witness(cs.clone(), || Ok(self.block.block_number - EPOCH_LENGTH))?;

        // Allocate all the inputs.
        let initial_state_commitment_var =
            FqVar::new_input(cs.clone(), || Ok(&self.initial_state_commitment))?;

        let final_state_commitment_var =
            FqVar::new_input(cs.clone(), || Ok(&self.final_state_commitment))?;

        // ------------------------- Process the inputs -------------------------------

        // Unpack the inputs by converting them from field elements to bits and truncating appropriately.
        let initial_state_commitment_bits =
            unpack_inputs(initial_state_commitment_var)?[..752].to_vec();

        let final_state_commitment_bits =
            unpack_inputs(final_state_commitment_var)?[..752].to_vec();

        // --------- Calculate the aggregate public key and the public key hash -------------

        // Initialize the field elements vector and the aggregate public key.
        let mut elems = vec![];
        let mut agg_pk = G1Var::zero();

        for (pk, included) in initial_pks_var.iter().zip(block_var.signer_bitmap.iter()) {
            // Calculate a new sum that includes the next public key.
            let new_sum = &agg_pk + pk;

            // Choose either the new public key sum or the old public key sum, depending on whether
            // the bitmap indicates that the validator signed or not.
            let cond_sum = CondSelectGadget::conditionally_select(included, &new_sum, &agg_pk)?;

            agg_pk = cond_sum;

            // Separate the key into field elements and add them to the elements vector to be hashed.
            elems.append(&mut pk.to_constraint_field()?);
        }

        // Calculate the Poseidon hash and serialize it.
        let initial_pk_hash =
            CRHGadget::<MNT4Fr>::evaluate(&poseidon_params_8_var, &elems)?.to_bits_be()?;

        // --------------- Verify witnesses against the public inputs --------------------

        // Verifying equality for initial state commitment. It just checks that the initial block
        // number, header hash and public key hash given as witnesses are correct by committing
        // to them and comparing the result with the initial state commitment given as an input.
        let mut reference_commitment = StateCommitmentGadget::evaluate(
            &initial_block_number_var,
            &initial_header_hash_var,
            &initial_pk_hash,
            &poseidon_params_2_var,
        )?;

        // We discard the last bit since input state commitment only contains 752 bits.
        reference_commitment.pop();

        initial_state_commitment_bits.enforce_equal(&reference_commitment)?;

        // Verifying equality for final state commitment. It just checks that the final block number,
        // header hash and public key hash given as a witnesses are correct by committing
        // to them and comparing the result with the final state commitment given as an input.
        let mut reference_commitment = StateCommitmentGadget::evaluate(
            &block_var.block_number,
            &block_var.header_hash,
            &block_var.pk_hash,
            &poseidon_params_2_var,
        )?;

        // We discard the last bit since input state commitment only contains 752 bits.
        reference_commitment.pop();

        final_state_commitment_bits.enforce_equal(&reference_commitment)?;

        // --------------- Verify block validity --------------------

        // Check that the initial block and the final block are exactly one epoch length apart.
        let calculated_block_number =
            UInt32::addmany(&[initial_block_number_var.clone(), epoch_length_var])?;

        calculated_block_number.enforce_equal(&block_var.block_number)?;

        // Verify that the block is valid.
        block_var
            .verify(cs, &agg_pk)?
            .enforce_equal(&Boolean::constant(true))?;

        Ok(())
    }
}
