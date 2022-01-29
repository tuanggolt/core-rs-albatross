use ark_mnt4_753::Fr as MNT4Fr;
use ark_mnt6_753::constraints::{FqVar, G1Var};
use ark_mnt6_753::{Fq, G1Projective};
use ark_r1cs_std::prelude::{AllocVar, Boolean, CondSelectGadget, CurveVar, EqGadget};
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};
use nimiq_bls::pedersen::pedersen_generators;
use nimiq_nano_primitives::{PK_TREE_BREADTH, PK_TREE_DEPTH};
use nimiq_primitives::policy::SLOTS;

use crate::gadgets::mnt4::SerializeGadget;
use crate::utils::unpack_inputs;

/// This is the leaf subcircuit of the PKTreeCircuit. This circuit main function is to process the
/// validator's public keys and "return" the aggregate public key for the Macro Block. At a
/// high-level, it divides all the computation into 2^n parts, where n is the depth of the tree, so
/// that each part uses only a manageable amount of memory and can be run on consumer hardware.
/// It does this by forming a binary tree of recursive SNARKs. Each of the 2^n leaves receives
/// Merkle tree commitments to the public keys list and a commitment to the corresponding aggregate
/// public key chunk (there are 2^n chunks, one for each leaf) in addition the position of the leaf
/// node on the tree (in little endian bits) and the part of the signer's bitmap relevant to the
/// leaf position. Each of the leaves then checks that its specific chunk of the public keys,
/// aggregated according to its specific chunk of the signer's bitmap, matches the corresponding
/// chunk of the aggregated public key.
/// All of the other upper levels of the recursive SNARK tree just verify SNARK proofs for its child
/// nodes and recursively aggregate the aggregate public key chunks (no pun intended).
/// At a lower-level, this circuit does two things:
///     1. That the public keys given as witness are a leaf of the Merkle tree of public keys, in a
///        specific position. The Merkle tree root and the position are given as inputs and the
///        Merkle proof is given as a witness.
///     2. That the public keys given as witness, when aggregated according to the signer's bitmap
///        (given as an input), match the aggregated public key commitment (also given as an input).
#[derive(Clone)]
pub struct PKTreeLeafCircuit {
    // Witnesses (private)
    pks: Vec<G1Projective>,

    // Inputs (public)
    // Our inputs are always vectors of booleans (semantically), so that they are consistent across
    // the different elliptic curves that we use. However, for compactness, we represent them as
    // field elements. Both of the curves that we use have a modulus of 753 bits and a capacity
    // of 752 bits. So, the first 752 bits (in little-endian) of each field element is data, and the
    // last bit is always set to zero.
    signer_bitmap_chunk: Fq,
}

impl PKTreeLeafCircuit {
    pub fn new(pks: Vec<G1Projective>, signer_bitmap: Fq) -> Self {
        Self {
            pks,
            signer_bitmap_chunk: signer_bitmap,
        }
    }
}

impl ConstraintSynthesizer<MNT4Fr> for PKTreeLeafCircuit {
    /// This function generates the constraints for the circuit.
    fn generate_constraints(self, cs: ConstraintSystemRef<MNT4Fr>) -> Result<(), SynthesisError> {
        // Allocate all the witnesses.
        let pks_var = Vec::<G1Var>::new_witness(cs.clone(), || Ok(&self.pks[..]))?;

        // Allocate all the inputs.
        let signer_bitmap_chunk_var =
            FqVar::new_input(cs.clone(), || Ok(&self.signer_bitmap_chunk))?;

        // Unpack the inputs by converting them from field elements to bits and truncating appropriately.
        let signer_bitmap_chunk_bits = unpack_inputs(vec![signer_bitmap_chunk_var])?
            [..SLOTS as usize / PK_TREE_BREADTH]
            .to_vec();

        //
        let mut bits = vec![];

        for item in pks_var.iter().take(self.pks.len()) {
            bits.extend(SerializeGadget::serialize_g1(cs.clone(), item)?);
        }

        // Calculate the aggregate public key.
        let mut calculated_agg_pk = G1Var::zero();

        for (pk, included) in pks_var.iter().zip(signer_bitmap_chunk_bits.iter()) {
            // Calculate a new sum that includes the next public key.
            let new_sum = &calculated_agg_pk + pk;

            // Choose either the new public key sum or the old public key sum, depending on whether
            // the bitmap indicates that the validator signed or not.
            let cond_sum =
                CondSelectGadget::conditionally_select(included, &new_sum, &calculated_agg_pk)?;

            calculated_agg_pk = cond_sum;
        }

        Ok(())
    }
}
