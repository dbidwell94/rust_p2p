use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

use crate::p2p_client::{IntoId, P2PClient};
use anyhow::{anyhow, Result as AResult};
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data_channel::RTCDataChannel;
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
}

impl<'a> P2PConnection<'a> {
    /// Creates a new `P2PConnection` from a `&P2PClient`.
    /// This is an async function, and expects the client to have at least one valid STUN server
    /// already setup
    pub async fn new(client: &'a P2PClient<'a>) -> AResult<Self> {
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
            .create_data_channel(&format!("data_channel_{}", client.id.id()), None)
            .await?;

        let (sx, rx) = channel();

        let id = client.id.id();
        data_channel.on_message(Box::new(move |msg| {
            let _ = sx.send(msg);
            Box::pin(async {})
        }));

        data_channel.on_open(Box::new(|| {
            println!("Data channel has opened");

            Box::pin(async {})
        }));

        connection.on_negotiation_needed(Box::new(move || {
            println!("need on_negotiation_needed: {0}", id);

            Box::pin(async {})
        }));

        connection.on_peer_connection_state_change(Box::new(|state| {
            println!("{state:?}");
            Box::pin(async {})
        }));

        Ok(Self {
            local_id: &client.id,
            data_channel,
            connection,
            remote_id: None,
            message_reciever: rx,
        })
    }

    /// Gets the offer for use with the signaling server
    pub(crate) async fn get_offer(&self) -> AResult<RTCSessionDescription> {
        let offer = self.connection.create_offer(None).await?;
        self.connection.set_local_description(offer).await?;

        let mut recv = self.connection.gathering_complete_promise().await;
        let _ = recv.recv().await;

        let local_description = self
            .connection
            .local_description()
            .await
            .ok_or(anyhow!("Unable to get local description"))?;

        Ok(local_description)
    }

    pub async fn set_remote_offer(&self, offer: RTCSessionDescription) -> AResult<()> {
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

        let mut recv = self.connection.gathering_complete_promise().await;

        self.connection.set_local_description(answer).await?;

        let _ = recv.recv().await;

        let local_description = self
            .connection
            .local_description()
            .await
            .ok_or(anyhow!("Unable to get local description"))?;

        Ok(local_description)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use webrtc::peer_connection::sdp::sdp_type::RTCSdpType;

    const STUN_SERVERS: [&str; 1] = ["stun:stun.l.google.com:19302"];

    #[tokio::test]
    async fn test_new_p2p_connection() -> AResult<()> {
        let client = P2PClient::new(STUN_SERVERS);
        let _ = P2PConnection::new(&client).await?;
        Ok(())
    }

    #[tokio::test]
    async fn test_get_local_description() -> AResult<()> {
        let client = P2PClient::new(STUN_SERVERS);
        let connection = P2PConnection::new(&client).await?;

        let offer = connection.get_offer().await?;
        assert_eq!(offer.sdp_type, RTCSdpType::Offer);
        Ok(())
    }

    #[tokio::test]
    async fn test_offer_answer() -> AResult<()> {
        let mut client1 = P2PClient::default();
        client1.id = Box::new("client1".to_owned());
        let mut client2 = P2PClient::default();
        client2.id = Box::new("client2".to_owned());

        let connection1 = P2PConnection::new(&client1).await?;
        let connection2 = P2PConnection::new(&client2).await?;

        let offer = connection1.get_offer().await?;
        assert_eq!(offer.sdp_type, RTCSdpType::Offer);

        let answer = connection2.get_answer(offer).await?;
        assert_eq!(answer.sdp_type, RTCSdpType::Answer);

        connection1.set_remote_offer(answer).await?;

        Ok(())
    }
}
