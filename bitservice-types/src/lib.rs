use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PeerReadRequest {
    pub request_id: Uuid,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PeerReadResponse {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadRequest {}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ReadResponse {
    pub response0: PeerReadResponse,
    pub response1: PeerReadResponse,
    pub response2: PeerReadResponse,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PeerWriteRequest {
    pub request_id: Uuid,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PeerWriteResponse {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteRequest {}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct WriteResponse {
    pub response0: PeerWriteResponse,
    pub response1: PeerWriteResponse,
    pub response2: PeerWriteResponse,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PeerPruneRequest {
    pub request_id: Uuid,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PeerPruneResponse {}
