use ark_ec::ProjectiveCurve;
use ark_mnt4_753::{
    Fq as MNT4Fq, G1Projective as MNT4G1Projective, G2Projective as MNT4G2Projective,
};
use ark_mnt6_753::{
    Fq as MNT6Fq, G1Projective as MNT6G1Projective, G2Projective as MNT6G2Projective,
};

use nimiq_bls::compression::BeSerialize;

/// Serializes a field element in the MNT4-753 curve.
pub fn serialize_fq_mnt4(elem: &MNT4Fq) -> [u8; 95] {
    let mut buffer = [0u8; 95];
    BeSerialize::serialize(elem, &mut &mut buffer[..]).unwrap();
    buffer
}

/// Serializes a field element in the MNT6-753 curve.
pub fn serialize_fq_mnt6(elem: &MNT6Fq) -> [u8; 95] {
    let mut buffer = [0u8; 95];
    BeSerialize::serialize(elem, &mut &mut buffer[..]).unwrap();
    buffer
}

/// Serializes a G1 point in the MNT4-753 curve.
pub fn serialize_g1_mnt4(point: &MNT4G1Projective) -> [u8; 95] {
    let mut buffer = [0u8; 95];
    BeSerialize::serialize(&point.into_affine(), &mut &mut buffer[..]).unwrap();
    buffer
}

/// Serializes a G2 point in the MNT4-753 curve.
pub fn serialize_g2_mnt4(point: &MNT4G2Projective) -> [u8; 190] {
    let mut buffer = [0u8; 190];
    BeSerialize::serialize(&point.into_affine(), &mut &mut buffer[..]).unwrap();
    buffer
}

/// Serializes a G1 point in the MNT6-753 curve.
pub fn serialize_g1_mnt6(point: &MNT6G1Projective) -> [u8; 95] {
    let mut buffer = [0u8; 95];
    BeSerialize::serialize(&point.into_affine(), &mut &mut buffer[..]).unwrap();
    buffer
}

/// Serializes a G2 point in the MNT6-753 curve.
pub fn serialize_g2_mnt6(point: &MNT6G2Projective) -> [u8; 285] {
    let mut buffer = [0u8; 285];
    BeSerialize::serialize(&point.into_affine(), &mut &mut buffer[..]).unwrap();
    buffer
}
