use serde::{Deserialize, Serialize};
use webrtc::{
    ice_transport::ice_candidate::RTCIceCandidate,
    peer_connection::sdp::session_description::RTCSessionDescription,
};

#[derive(Serialize, Deserialize)]
pub struct BroadcastCandidateArgs {
    pub candidates: Vec<RTCIceCandidate>,
    pub session_description: Option<RTCSessionDescription>,
}
