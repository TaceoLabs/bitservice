use mpc_core::protocols::rep3::Rep3PrimeFieldShare;
use serde::{Deserialize, Serialize};

use crate::groth16::Groth16Proof;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerReadRequest {
    pub key: String,
    pub r: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerReadResponse {
    pub value: Rep3PrimeFieldShare<ark_bn254::Fr>,
    pub proof: Groth16Proof,
    #[serde(serialize_with = "ark_serde_compat::serialize_bn254_fr")]
    #[serde(deserialize_with = "ark_serde_compat::deserialize_bn254_fr")]
    pub root: ark_bn254::Fr,
    #[serde(serialize_with = "ark_serde_compat::serialize_bn254_fr")]
    #[serde(deserialize_with = "ark_serde_compat::deserialize_bn254_fr")]
    pub commitment: ark_bn254::Fr,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadRequest {
    pub requests: [PeerReadRequest; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadResponse {
    pub responses: [PeerReadResponse; 3],
}
