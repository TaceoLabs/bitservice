use serde::{Deserialize, Serialize};

use crate::groth16::Groth16Proof;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerBanRequest {
    pub key: String,
    pub value: String,
    pub r_key: String,
    pub r_value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerBanResponse {
    pub proof: Groth16Proof,
    #[serde(serialize_with = "ark_serde_compat::serialize_bn254_fr")]
    #[serde(deserialize_with = "ark_serde_compat::deserialize_bn254_fr")]
    pub old_root: ark_bn254::Fr,
    #[serde(serialize_with = "ark_serde_compat::serialize_bn254_fr")]
    #[serde(deserialize_with = "ark_serde_compat::deserialize_bn254_fr")]
    pub new_root: ark_bn254::Fr,
    #[serde(serialize_with = "ark_serde_compat::serialize_bn254_fr")]
    #[serde(deserialize_with = "ark_serde_compat::deserialize_bn254_fr")]
    pub commitment_key: ark_bn254::Fr,
    #[serde(serialize_with = "ark_serde_compat::serialize_bn254_fr")]
    #[serde(deserialize_with = "ark_serde_compat::deserialize_bn254_fr")]
    pub commitment_value: ark_bn254::Fr,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BanRequest {
    pub requests: [PeerBanRequest; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BanResponse {
    pub responses: [PeerBanResponse; 3],
}
