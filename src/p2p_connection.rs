use crate::signal_server::{SignalServerClient, SignalServerError};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::mpsc::channel;
use uuid::Uuid;
use webrtc::{
    data_channel::RTCDataChannel, ice_transport::ice_candidate::RTCIceCandidate,
    peer_connection::RTCPeerConnection, Error as RTCError,
};

#[derive(Error, Debug)]
pub enum P2PConnectionError {
    #[error("Failed to create data channel")]
    P2PConnectionCreationFailed(#[from] RTCError),
    #[error("The connection attempt was cancelled")]
    ConnectionCancelled,
    #[error("Failed to get ICE candidates")]
    SignalServerError(#[from] SignalServerError),
}

#[derive(Clone, Debug)]
pub struct RoomInfo {
    pub room: String,
    pub channel: String,
}

pub struct P2PConnection {
    id: Uuid,
    data_channel: Arc<RTCDataChannel>,
    peer_connection: RTCPeerConnection,
}

impl P2PConnection {
    pub async fn from_peer_connection(
        peer_connection: RTCPeerConnection,
        signal_server_client: Arc<SignalServerClient>,
        room: RoomInfo,
    ) -> Result<Self, P2PConnectionError> {
        let p2p_id = Uuid::new_v4();

        let data_channel = peer_connection
            .create_data_channel(&format!("data-channel-{p2p_id}"), None)
            .await?;

        let (tx_connection_state, rx_connection_state) = channel(2);

        peer_connection.on_peer_connection_state_change(Box::new(move |state| {
            let _ = tx_connection_state.try_send(state);

            Box::pin(async {})
        }));

        let offer = peer_connection.create_offer(None).await?;
        peer_connection.set_local_description(offer).await?;

        Self::announce_ice_candidates(
            &peer_connection,
            &signal_server_client,
            p2p_id.to_string(),
            &room,
        )
        .await?;

        // TODO send the candidates to the other peer
        todo!("Finish this implementation");

        Ok(Self {
            id: p2p_id,
            data_channel: data_channel,
            peer_connection,
        })
    }

    async fn announce_ice_candidates(
        peer_connection: &RTCPeerConnection,
        signal_server_client: &Arc<SignalServerClient>,
        p2p_id: String,
        room_info: &RoomInfo,
    ) -> Result<(), P2PConnectionError> {
        // Create a channel to receive ICE candidates. Set the buffer to 2 to ensure that we don't accidentally miss a candidate.
        let (tx_ice_candidates, mut rx_ice_candidates) = channel(2);

        peer_connection.on_ice_candidate(Box::new(move |candidate| {
            let _ = tx_ice_candidates.try_send(candidate);

            Box::pin(async {})
        }));

        // Init this to 5 to prevent reallocation. ICE candidates are usually around 5.
        let mut found_candidates: Vec<RTCIceCandidate> = Vec::with_capacity(5);

        // In this first branch, a `None` means that the tx_ice_candidates channel was closed
        while let Some(candidate) = rx_ice_candidates.recv().await {
            // In this branch, a `None` means that the ICE traversal is complete. We want the latest candidate.
            if let Some(candidate) = candidate {
                found_candidates.push(candidate);
            } else {
                break;
            }
        }

        signal_server_client
            .announce_self(
                &room_info.channel,
                &room_info.room,
                &p2p_id,
                &found_candidates,
            )
            .await?;

        Ok(())
    }

    async fn look_for_ice_candidates(
        peer_connection: &RTCPeerConnection,
        signal_server_client: &Arc<SignalServerClient>,
        p2p_id: String,
        room_info: &RoomInfo,
    ) -> Result<Vec<RTCIceCandidate>, P2PConnectionError> {
        let candidates = signal_server_client
            .get_candidates_in_room(&room_info.channel, &room_info.room)
            .await?;

        Ok(candidates)
    }
}

impl Drop for P2PConnection {
    fn drop(&mut self) {
        let _ = self.data_channel.close();
        let _ = self.peer_connection.close();
    }
}
