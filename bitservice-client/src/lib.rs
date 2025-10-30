use ark_bn254::Bn254;
use ark_ff::{AdditiveGroup, Field};
use ark_groth16::VerifyingKey;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use bitservice_types::{
    ban::{BanRequest, BanResponse, PeerBanRequest},
    read::{PeerReadRequest, ReadRequest, ReadResponse},
    unban::{PeerUnbanRequest, UnbanRequest, UnbanResponse},
};
use co_noir_to_r1cs::noir::r1cs;
use crypto_box::PublicKey;
use mpc_core::protocols::{
    rep3,
    rep3_ring::{self, ring::ring_impl::RingElement},
};
use rand::{CryptoRng, Rng};
use serde::{Deserialize, Serialize};

pub use reqwest;

#[cfg(feature = "bitservice-client-bin")]
pub mod config;

pub const NOT_BANNED: ark_bn254::Fr = ark_bn254::Fr::ZERO;
pub const BANNED: ark_bn254::Fr = ark_bn254::Fr::ONE;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Value {
    #[default]
    NotBanned,
    Banned,
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl TryFrom<ark_bn254::Fr> for Value {
    type Error = eyre::Report;

    fn try_from(value: ark_bn254::Fr) -> Result<Self, Self::Error> {
        if value == NOT_BANNED {
            Ok(Self::NotBanned)
        } else if value == BANNED {
            Ok(Self::Banned)
        } else {
            eyre::bail!("invalid value: {value}");
        }
    }
}

#[derive(Debug, Clone)]
pub struct Client {
    client: reqwest::Client,
    server_url: String,
    rp_id: u128,
    peer_public_keys: [PublicKey; 3],
    read_vk: VerifyingKey<Bn254>,
    write_vk: VerifyingKey<Bn254>,
}

impl Client {
    pub fn new(
        client: reqwest::Client,
        server_url: String,
        rp_id: u128,
        peer_public_keys: [PublicKey; 3],
        read_vk: VerifyingKey<Bn254>,
        write_vk: VerifyingKey<Bn254>,
    ) -> Self {
        Self {
            client,
            server_url,
            rp_id,
            peer_public_keys,
            read_vk,
            write_vk,
        }
    }

    pub async fn read<R: Rng + CryptoRng>(
        &self,
        key: u32,
        randomness_commitment: ark_bn254::Fr,
        rng: &mut R,
    ) -> eyre::Result<Value> {
        let key_shares = rep3_ring::share_ring_element_binary(RingElement(key), rng);
        let [key0, key1, key2] = serialize_encode_seal(key_shares, &self.peer_public_keys, rng);
        let r_shares = rep3::share_field_element(randomness_commitment, rng);
        let [r0, r1, r2] = serialize_encode_seal(r_shares, &self.peer_public_keys, rng);

        let req0 = PeerReadRequest { key: key0, r: r0 };
        let req1 = PeerReadRequest { key: key1, r: r1 };
        let req2 = PeerReadRequest { key: key2, r: r2 };
        let req = ReadRequest {
            requests: [req0, req1, req2],
        };

        let res = self
            .client
            .post(format!("{}/api/v1/read/{}", self.server_url, self.rp_id))
            .json(&req)
            .send()
            .await?;
        if !res.status().is_success() {
            let error = res.text().await?;
            eyre::bail!("server return error: {error}");
        }
        let ReadResponse {
            responses: [res0, res1, res2],
        } = res.json::<ReadResponse>().await?;

        let value = (res0.value + res1.value + res2.value).a;

        // verify the proofs
        assert!(r1cs::verify(
            &self.read_vk,
            &res0.proof.into(),
            &[res0.root, res0.commitment]
        )?);
        assert!(r1cs::verify(
            &self.read_vk,
            &res1.proof.into(),
            &[res1.root, res1.commitment]
        )?);
        assert!(r1cs::verify(
            &self.read_vk,
            &res2.proof.into(),
            &[res2.root, res2.commitment]
        )?);

        value.try_into()
    }

    pub async fn ban<R: Rng + CryptoRng>(
        &self,
        key: u32,
        randomness_key: ark_bn254::Fr,
        randomness_commitment: ark_bn254::Fr,
        rng: &mut R,
    ) -> eyre::Result<()> {
        let key_shares = rep3_ring::share_ring_element_binary(RingElement(key), rng);
        let [key0, key1, key2] = serialize_encode_seal(key_shares, &self.peer_public_keys, rng);
        let value_shares = rep3::share_field_element(BANNED, rng);
        let [value0, value1, value2] =
            serialize_encode_seal(value_shares, &self.peer_public_keys, rng);
        let r_key_shares = rep3::share_field_element(randomness_key, rng);
        let [r_key0, r_key1, r_key2] =
            serialize_encode_seal(r_key_shares, &self.peer_public_keys, rng);
        let r_value_shares = rep3::share_field_element(randomness_commitment, rng);
        let [r_value0, r_value1, r_value2] =
            serialize_encode_seal(r_value_shares, &self.peer_public_keys, rng);

        let req0 = PeerBanRequest {
            key: key0,
            value: value0,
            r_key: r_key0,
            r_value: r_value0,
        };
        let req1 = PeerBanRequest {
            key: key1,
            value: value1,
            r_key: r_key1,
            r_value: r_value1,
        };
        let req2 = PeerBanRequest {
            key: key2,
            value: value2,
            r_key: r_key2,
            r_value: r_value2,
        };
        let req = BanRequest {
            requests: [req0, req1, req2],
        };

        let res = self
            .client
            .post(format!("{}/api/v1/ban/{}", self.server_url, self.rp_id))
            .json(&req)
            .send()
            .await?;
        if !res.status().is_success() {
            let error = res.text().await?;
            eyre::bail!("server return error: {error}");
        }
        let BanResponse {
            responses: [res0, res1, res2],
        } = res.json::<BanResponse>().await?;

        // verify the proofs
        assert!(r1cs::verify(
            &self.write_vk,
            &res0.proof.into(),
            &[
                res0.old_root,
                res0.new_root,
                res0.commitment_key,
                res0.commitment_value
            ]
        )?);
        assert!(r1cs::verify(
            &self.write_vk,
            &res1.proof.into(),
            &[
                res1.old_root,
                res1.new_root,
                res1.commitment_key,
                res1.commitment_value
            ]
        )?);
        assert!(r1cs::verify(
            &self.write_vk,
            &res2.proof.into(),
            &[
                res2.old_root,
                res2.new_root,
                res2.commitment_key,
                res2.commitment_value
            ]
        )?);

        Ok(())
    }

    pub async fn unban<R: Rng + CryptoRng>(
        &self,
        key: u32,
        randomness_key: ark_bn254::Fr,
        randomness_commitment: ark_bn254::Fr,
        rng: &mut R,
    ) -> eyre::Result<()> {
        let key_shares = rep3_ring::share_ring_element_binary(RingElement(key), rng);
        let [key0, key1, key2] = serialize_encode_seal(key_shares, &self.peer_public_keys, rng);
        let value_shares = rep3::share_field_element(NOT_BANNED, rng);
        let [value0, value1, value2] =
            serialize_encode_seal(value_shares, &self.peer_public_keys, rng);
        let r_key_shares = rep3::share_field_element(randomness_key, rng);
        let [r_key0, r_key1, r_key2] =
            serialize_encode_seal(r_key_shares, &self.peer_public_keys, rng);
        let r_value_shares = rep3::share_field_element(randomness_commitment, rng);
        let [r_value0, r_value1, r_value2] =
            serialize_encode_seal(r_value_shares, &self.peer_public_keys, rng);

        let req0 = PeerUnbanRequest {
            key: key0,
            value: value0,
            r_key: r_key0,
            r_value: r_value0,
        };
        let req1 = PeerUnbanRequest {
            key: key1,
            value: value1,
            r_key: r_key1,
            r_value: r_value1,
        };
        let req2 = PeerUnbanRequest {
            key: key2,
            value: value2,
            r_key: r_key2,
            r_value: r_value2,
        };
        let req = UnbanRequest {
            requests: [req0, req1, req2],
        };

        let res = self
            .client
            .post(format!("{}/api/v1/unban/{}", self.server_url, self.rp_id))
            .json(&req)
            .send()
            .await?;
        if !res.status().is_success() {
            let error = res.text().await?;
            eyre::bail!("server return error: {error}");
        }
        let UnbanResponse {
            responses: [res0, res1, res2],
        } = res.json::<UnbanResponse>().await?;

        // verify the proofs
        assert!(r1cs::verify(
            &self.write_vk,
            &res0.proof.into(),
            &[
                res0.old_root,
                res0.new_root,
                res0.commitment_key,
                res0.commitment_value
            ]
        )?);
        assert!(r1cs::verify(
            &self.write_vk,
            &res1.proof.into(),
            &[
                res1.old_root,
                res1.new_root,
                res1.commitment_key,
                res1.commitment_value
            ]
        )?);
        assert!(r1cs::verify(
            &self.write_vk,
            &res2.proof.into(),
            &[
                res2.old_root,
                res2.new_root,
                res2.commitment_key,
                res2.commitment_value
            ]
        )?);

        Ok(())
    }
}

fn serialize_encode_seal<T: Serialize, R: Rng + CryptoRng>(
    shares: [T; 3],
    peer_public_keys: &[PublicKey],
    rng: &mut R,
) -> [String; 3] {
    shares
        .into_iter()
        .zip(peer_public_keys)
        .map(|(share, pk)| {
            let ct = pk
                .seal(
                    rng,
                    &bincode::serde::encode_to_vec(share, bincode::config::standard())
                        .expect("can serialize"),
                )
                .expect("can seal");
            STANDARD.encode(ct)
        })
        .collect::<Vec<_>>()
        .try_into()
        .expect("len is 3")
}
