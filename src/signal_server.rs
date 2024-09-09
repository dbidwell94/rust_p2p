use anyhow::Result as AResult;
use reqwest::Url;
use signal_server::BroadcastCandidateArgs;
use webrtc::{
    ice_transport::ice_candidate::RTCIceCandidate,
    peer_connection::sdp::session_description::RTCSessionDescription,
};

#[derive(Debug, Clone)]
pub struct RoomConfig {
    pub room: String,
    pub channel: String,
}

pub struct SignalServer {
    url: Url,
    room_config: RoomConfig,
    client: reqwest::Client,
}

impl SignalServer {
    pub fn new(url: Url, room_config: RoomConfig) -> Self {
        let client = reqwest::Client::new();

        Self {
            url,
            room_config,
            client,
        }
    }

    pub async fn broadcast_self(
        &self,
        peer_id: String,
        session_description: &RTCSessionDescription,
        ice_candidates: Vec<RTCIceCandidate>,
    ) -> AResult<()> {
        let url = self.url.join("/announce")?;

        let query = vec![
            ("channel", self.room_config.channel.as_str()),
            ("room", self.room_config.room.as_str()),
            ("peer_id", peer_id.as_str()),
        ];

        let body = BroadcastCandidateArgs {
            candidates: ice_candidates,
            session_description: Some(session_description.clone()),
        };

        self.client
            .post(url)
            .query(&query)
            .json(&body)
            .send()
            .await?;

        Ok(())
    }
}
