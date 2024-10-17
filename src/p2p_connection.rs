use crate::p2p_client::{IntoId, P2PClient};
use anyhow::{anyhow, Result as AResult};
use core::task;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::{channel, Receiver};
use webrtc::data_channel::data_channel_init::RTCDataChannelInit;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_candidate::{RTCIceCandidate, RTCIceCandidateInit};
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;

pub struct P2PConnection<'a> {
    connection: RTCPeerConnection,
    data_channel: Arc<RTCDataChannel>,
    local_id: &'a Box<dyn IntoId>,
    remote_id: Option<Box<dyn IntoId>>,
    message_reciever: Receiver<DataChannelMessage>,
    ice_candidates: Arc<RwLock<Vec<RTCIceCandidate>>>,
    connected: Arc<AtomicBool>,
}

impl<'a> std::fmt::Debug for P2PConnection<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("P2PConnection: {}", self.local_id.id()))
    }
}

impl<'a> P2PConnection<'a> {
    /// Creates a new `P2PConnection` from a `&P2PClient`.
    /// This is an async function, and expects the client to have at least one valid STUN server
    /// already setup
    ///
    /// * `client` - The P2P Client which will take control of this struct
    /// * `require_reliable_transmission` - if `true`, then we require ordered packets. This makes
    /// our packets more reliable, but at the potential cost of network performance as we do not
    /// allow dropped packets
    pub async fn new(
        client: &'a P2PClient<'a>,
        require_reliable_transmission: bool,
    ) -> AResult<Self> {
        let config = RTCConfiguration {
            ice_servers: client
                .ice_servers
                .clone()
                .into_iter()
                .map(|server| RTCIceServer {
                    urls: vec![server],
                    ..Default::default()
                })
                .collect::<Vec<_>>(),
            ..Default::default()
        };

        let connection = client.api.new_peer_connection(config).await?;
        let data_channel = connection
            .create_data_channel(
                &format!("data_channel_{}", client.id.id()),
                Some(RTCDataChannelInit {
                    ordered: Some(require_reliable_transmission),
                    ..Default::default()
                }),
            )
            .await?;

        let (sx, rx) = channel(128);

        data_channel.on_message(Box::new(move |msg| {
            let sx = sx.clone();
            Box::pin(async move {
                let _ = sx.send(msg).await;
            })
        }));

        let connected = Arc::new(AtomicBool::new(false));
        let connected_clone = connected.clone();
        connection.on_peer_connection_state_change(Box::new(move |state| {
            match state {
                webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState::Connected => {
                    connected_clone.store(true, std::sync::atomic::Ordering::Relaxed);
                },
                _ => {
                    connected_clone.store(false, std::sync::atomic::Ordering::Relaxed);
                }
            };


            Box::pin(async {})
        }));

        let ice_candidates = Arc::new(RwLock::new(Vec::new()));

        let candidates_clone = ice_candidates.clone();

        connection.on_ice_candidate(Box::new(move |candidate| {
            let cloned = candidates_clone.clone();

            Box::pin(async move {
                if let Some(candidate) = candidate {
                    let mut candidates = cloned.write().expect("Unable to aquire write lock");
                    candidates.push(candidate);
                }
            })
        }));

        Ok(Self {
            local_id: &client.id,
            data_channel,
            connection,
            remote_id: None,
            message_reciever: rx,
            ice_candidates,
            connected,
        })
    }

    /// Gets the offer for use with the signaling server
    /// Will also trickle ICE candidates and automatically send them to the signaling server so the
    /// other peer can add them in turn
    pub(crate) async fn get_offer(&self) -> AResult<RTCSessionDescription> {
        let offer = self.connection.create_offer(None).await?;
        self.connection.set_local_description(offer).await?;

        let local_description = self
            .connection
            .local_description()
            .await
            .ok_or(anyhow!("Unable to get local description"))?;

        Ok(local_description)
    }

    pub(crate) async fn set_answer(&self, offer: RTCSessionDescription) -> AResult<()> {
        self.connection.set_remote_description(offer).await?;
        Ok(())
    }

    /// Used to set the remote answer to the connection
    pub(crate) async fn get_answer(
        &self,
        offer: RTCSessionDescription,
    ) -> AResult<RTCSessionDescription> {
        self.connection.set_remote_description(offer).await?;

        let answer = self.connection.create_answer(None).await?;

        self.connection.set_local_description(answer).await?;

        let local_description = self
            .connection
            .local_description()
            .await
            .ok_or(anyhow!("Unable to get local description"))?;

        Ok(local_description)
    }

    pub(crate) async fn set_candidates(
        &self,
        candidates: impl Iterator<Item = RTCIceCandidateInit>,
    ) -> AResult<()> {
        for candidate in candidates {
            self.connection.add_ice_candidate(candidate).await?;
        }
        Ok(())
    }

    /// Gets all of the not-yet-gotten ICE Candidates from the queue, for use with sending through
    /// the signaling server
    pub(crate) fn get_pending_candidates(&self) -> AResult<Vec<RTCIceCandidate>> {
        Ok(self
            .ice_candidates
            .read()
            .map_err(|_| anyhow!("Unable to aquire read lock guard"))?
            .clone())
    }

    pub(crate) fn get_is_connected_to_peer(&self) -> bool {
        self.connected.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl<'a> Drop for P2PConnection<'a> {
    fn drop(&mut self) {
        futures::executor::block_on(async move {
            let _ = self.data_channel.close().await;
            println!("Data Channel has been closed");
            let _ = self.connection.close().await;
            println!("Connection has been closed");
        });
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use tokio::time::{sleep, Instant};
    use webrtc::peer_connection::sdp::sdp_type::RTCSdpType;

    const STUN_SERVERS: [&str; 1] = ["stun:stun.l.google.com:19302"];

    async fn wait_for_condition<'a>(
        condition: Box<dyn Fn() -> AResult<bool> + 'a>,
        timeout: Duration,
    ) -> AResult<()> {
        let now = Instant::now();

        while now.elapsed() < timeout {
            if condition()? {
                return Ok(());
            }
            sleep(Duration::from_millis(10)).await;
        }
        return Err(anyhow!("Unable to validate condition"));
    }

    #[tokio::test]
    async fn test_new_p2p_connection() -> AResult<()> {
        let client = P2PClient::new(STUN_SERVERS);
        let _ = P2PConnection::new(&client, true).await?;
        Ok(())
    }

    #[tokio::test]
    async fn test_get_local_description() -> AResult<()> {
        let client = P2PClient::new(STUN_SERVERS);
        let connection = P2PConnection::new(&client, true).await?;

        let offer = connection.get_offer().await?;
        assert_eq!(offer.sdp_type, RTCSdpType::Offer);
        Ok(())
    }

    #[tokio::test]
    async fn test_facilitate_p2p_connection() -> AResult<()> {
        let client1 = P2PClient::new(STUN_SERVERS);
        let client2 = P2PClient::new(STUN_SERVERS);

        let connection1 = Arc::new(P2PConnection::new(&client1, true).await?);
        let connection2 = Arc::new(P2PConnection::new(&client2, true).await?);

        let offer = connection1.get_offer().await?;
        assert_eq!(offer.sdp_type, RTCSdpType::Offer);

        let answer = connection2.get_answer(offer).await?;
        assert_eq!(answer.sdp_type, RTCSdpType::Answer);

        connection1.set_answer(answer).await?;

        {
            let con_clone = connection1.clone();
            wait_for_condition(
                Box::new(move || Ok(con_clone.get_pending_candidates()?.len() > 0)),
                Duration::from_secs(10),
            )
            .await?;
        }
        {
            let con_clone = connection2.clone();
            wait_for_condition(
                Box::new(move || Ok(con_clone.get_pending_candidates()?.len() > 0)),
                Duration::from_secs(10),
            )
            .await?;
        }

        let con1_candidates = connection1.get_pending_candidates()?;
        let con2_candidates = connection2.get_pending_candidates()?;

        connection1
            .set_candidates(con2_candidates.iter().map(|can| {
                can.to_json()
                    .expect("Unable to convert RTCIceCandidate to RTCIceCandidateInit")
            }))
            .await?;

        connection2
            .set_candidates(con1_candidates.iter().map(|can| {
                can.to_json()
                    .expect("Unable to convert RTCIceCandidate to RTCIceCandidateInit")
            }))
            .await?;

        {
            let (con1, con2) = (connection1.clone(), connection2.clone());

            wait_for_condition(
                Box::new(move || Ok(con1.get_is_connected_to_peer())),
                Duration::from_secs(10),
            )
            .await?;
            wait_for_condition(
                Box::new(move || Ok(con2.get_is_connected_to_peer())),
                Duration::from_secs(10),
            )
            .await?;
        }

        assert!(connection1.get_is_connected_to_peer());
        assert!(connection2.get_is_connected_to_peer());

        Ok(())
    }
}
