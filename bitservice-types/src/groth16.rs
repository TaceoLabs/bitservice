use ark_bn254::Bn254;
use ark_groth16::VerifyingKey;
use serde::{Deserialize, Serialize};

/// A proof in the Groth16 SNARK.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Groth16Proof {
    /// The `A` element in `G1`.
    #[serde(rename = "pi_a")]
    #[serde(serialize_with = "ark_serde_compat::serialize_bn254_g1")]
    #[serde(deserialize_with = "ark_serde_compat::deserialize_bn254_g1")]
    pub a: ark_bn254::G1Affine,
    /// The `B` element in `G2`.
    #[serde(rename = "pi_b")]
    #[serde(serialize_with = "ark_serde_compat::serialize_bn254_g2")]
    #[serde(deserialize_with = "ark_serde_compat::deserialize_bn254_g2")]
    pub b: ark_bn254::G2Affine,
    /// The `C` element in `G1`.
    #[serde(rename = "pi_c")]
    #[serde(serialize_with = "ark_serde_compat::serialize_bn254_g1")]
    #[serde(deserialize_with = "ark_serde_compat::deserialize_bn254_g1")]
    pub c: ark_bn254::G1Affine,
}

impl From<Groth16Proof> for ark_groth16::Proof<Bn254> {
    fn from(value: Groth16Proof) -> Self {
        Self {
            a: value.a,
            b: value.b,
            c: value.c,
        }
    }
}

impl From<ark_groth16::Proof<Bn254>> for Groth16Proof {
    fn from(value: ark_groth16::Proof<Bn254>) -> Self {
        Self {
            a: value.a,
            b: value.b,
            c: value.c,
        }
    }
}

/// Represents a verification key in JSON format that was created by circom. Supports de/serialization using [`serde`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Groth16VerificationKey {
    /// The protocol used to generate the proof (always `"groth16"`)
    pub protocol: String,
    /// The curve
    pub curve: String,
    /// The number of public inputs
    #[serde(rename = "nPublic")]
    pub n_public: usize,
    /// The element α of the verification key ∈ G1
    #[serde(rename = "vk_alpha_1")]
    #[serde(serialize_with = "ark_serde_compat::serialize_bn254_g1")]
    #[serde(deserialize_with = "ark_serde_compat::deserialize_bn254_g1")]
    pub alpha_1: ark_bn254::G1Affine,
    /// The element β of the verification key ∈ G2
    #[serde(rename = "vk_beta_2")]
    #[serde(serialize_with = "ark_serde_compat::serialize_bn254_g2")]
    #[serde(deserialize_with = "ark_serde_compat::deserialize_bn254_g2")]
    pub beta_2: ark_bn254::G2Affine,
    /// The γ of the verification key ∈ G2
    #[serde(rename = "vk_gamma_2")]
    #[serde(serialize_with = "ark_serde_compat::serialize_bn254_g2")]
    #[serde(deserialize_with = "ark_serde_compat::deserialize_bn254_g2")]
    pub gamma_2: ark_bn254::G2Affine,
    /// The element δ of the verification key ∈ G2
    #[serde(rename = "vk_delta_2")]
    #[serde(serialize_with = "ark_serde_compat::serialize_bn254_g2")]
    #[serde(deserialize_with = "ark_serde_compat::deserialize_bn254_g2")]
    pub delta_2: ark_bn254::G2Affine,
    /// The pairing of α and β of the verification key ∈ Gt
    #[serde(rename = "vk_alphabeta_12")]
    #[serde(serialize_with = "ark_serde_compat::serialize_bn254_gt")]
    #[serde(deserialize_with = "ark_serde_compat::deserialize_bn254_gt")]
    pub alpha_beta_gt: ark_bn254::Fq12,
    /// Used to bind the public inputs to the proof
    #[serde(rename = "IC")]
    #[serde(serialize_with = "ark_serde_compat::serialize_bn254_g1_sequence")]
    #[serde(deserialize_with = "ark_serde_compat::deserialize_bn254_g1_sequence")]
    pub ic: Vec<ark_bn254::G1Affine>,
}

impl From<Groth16VerificationKey> for VerifyingKey<Bn254> {
    fn from(vk: Groth16VerificationKey) -> Self {
        VerifyingKey {
            alpha_g1: vk.alpha_1,
            beta_g2: vk.beta_2,
            gamma_g2: vk.gamma_2,
            delta_g2: vk.delta_2,
            gamma_abc_g1: vk.ic,
        }
    }
}
