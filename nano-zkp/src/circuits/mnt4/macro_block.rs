use ark_crypto_primitives::snark::BooleanInputVar;
use ark_crypto_primitives::SNARKGadget;
use ark_groth16::constraints::{Groth16VerifierGadget, ProofVar, VerifyingKeyVar};
use ark_groth16::{Proof, VerifyingKey};
use ark_mnt4_753::Fr as MNT4Fr;
use ark_mnt6_753::constraints::{FqVar, G1Var, PairingVar};
use ark_mnt6_753::{Fq, G1Projective, MNT6_753};
use ark_r1cs_std::prelude::{
    AllocVar, Boolean, CurveVar, EqGadget, FieldVar, ToBitsGadget, UInt32,
};
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};
use nimiq_bls::pedersen::pedersen_generators;
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
    final_pk_tree_root: Vec<bool>,
    block: MacroBlock,

    // Inputs (public)
    // Our inputs are always vectors of booleans (semantically), so that they are consistent across
    // the different elliptic curves that we use. However, for compactness, we represent them as
    // field elements. Both of the curves that we use have a modulus of 753 bits and a capacity
    // of 752 bits. So, the first 752 bits (in little-endian) of each field element is data, and the
    // last bit is always set to zero.
    initial_state_commitment: Vec<Fq>,
    final_state_commitment: Vec<Fq>,
}

impl MacroBlockCircuit {
    pub fn new(
        initial_pk_tree_root: Vec<bool>,
        initial_header_hash: Vec<bool>,
        final_pk_tree_root: Vec<bool>,
        block: MacroBlock,
        initial_state_commitment: Vec<Fq>,
        final_state_commitment: Vec<Fq>,
    ) -> Self {
        Self {
            initial_pk_tree_root,
            initial_header_hash,
            final_pk_tree_root,
            block,
            initial_state_commitment,
            final_state_commitment,
        }
    }
}

impl ConstraintSynthesizer<MNT4Fr> for MacroBlockCircuit {
    /// This function generates the constraints for the circuit.
    fn generate_constraints(self, cs: ConstraintSystemRef<MNT4Fr>) -> Result<(), SynthesisError> {
        // Allocate all the constants.
        let epoch_length_var = UInt32::<MNT4Fr>::new_constant(cs.clone(), EPOCH_LENGTH)?;

        // Allocate all the witnesses.
        let initial_pk_tree_root_var =
            Vec::<Boolean<MNT4Fr>>::new_witness(cs.clone(), || Ok(&self.initial_pk_tree_root[..]))?;

        let initial_header_hash_var =
            Vec::<Boolean<MNT4Fr>>::new_witness(cs.clone(), || Ok(&self.initial_header_hash[..]))?;

        let final_pk_tree_root_var =
            Vec::<Boolean<MNT4Fr>>::new_witness(cs.clone(), || Ok(&self.final_pk_tree_root[..]))?;

        let block_var = MacroBlockGadget::new_witness(cs.clone(), || Ok(&self.block))?;

        let initial_block_number_var =
            UInt32::new_witness(cs.clone(), || Ok(self.block.block_number - EPOCH_LENGTH))?;

        // Allocate all the inputs.
        let initial_state_commitment_var =
            Vec::<FqVar>::new_input(cs.clone(), || Ok(&self.initial_state_commitment[..]))?;

        let final_state_commitment_var =
            Vec::<FqVar>::new_input(cs.clone(), || Ok(&self.final_state_commitment[..]))?;

        // Unpack the inputs by converting them from field elements to bits and truncating appropriately.
        let initial_state_commitment_bits =
            unpack_inputs(initial_state_commitment_var)?[..760].to_vec();

        let final_state_commitment_bits =
            unpack_inputs(final_state_commitment_var)?[..760].to_vec();

        // Check that the initial block and the final block are exactly one epoch length apart.
        let calculated_block_number =
            UInt32::addmany(&[initial_block_number_var.clone(), epoch_length_var])?;

        calculated_block_number.enforce_equal(&block_var.block_number)?;

        final_state_commitment_bits.enforce_equal(&reference_commitment)?;

        // Verifying that the block is valid.
        block_var
            .verify(cs, &final_pk_tree_root_var, &agg_pk_var)?
            .enforce_equal(&Boolean::constant(true))?;

        // Verifying equality for initial state commitment. It just checks that the initial block
        // number, header hash and public key tree root given as witnesses are correct by committing
        // to them and comparing the result with the initial state commitment given as an input.
        let reference_commitment = StateCommitmentGadget::evaluate(
            &initial_block_number_var,
            &initial_header_hash_var,
            &initial_pk_tree_root_var,
            &pedersen_generators_var,
        )?;

        initial_state_commitment_bits.enforce_equal(&reference_commitment)?;

        // Verifying equality for final state commitment. It just checks that the final block number,
        // header hash and public key tree root given as a witnesses are correct by committing
        // to them and comparing the result with the final state commitment given as an input.
        let reference_commitment = StateCommitmentGadget::evaluate(
            &block_var.block_number,
            &block_var.header_hash,
            &final_pk_tree_root_var,
            &pedersen_generators_var,
        )?;

        Ok(())
    }
}
