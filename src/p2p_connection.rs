use crate::signal_server::{RoomConfig, SignalServer};

use anyhow::{anyhow, Result as AResult};
use reqwest::Url;
use std::{hash::Hash, sync::Arc};
use tokio::sync::mpsc::channel;
use uuid::Uuid;
use webrtc::{
    api::API,
    data_channel::RTCDataChannel,
    ice_transport::ice_server::RTCIceServer,
    peer_connection::{configuration::RTCConfiguration, RTCPeerConnection},
};

pub struct P2PConnection {
    id: Uuid,
    connection: RTCPeerConnection,
    data_channel: Arc<RTCDataChannel>,
}

impl Hash for P2PConnection {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl P2PConnection {
    pub(crate) async fn from_api(ice_connections: &[String], api: Arc<API>) -> AResult<Self> {
        let config = RTCConfiguration {
            ice_servers: vec![RTCIceServer {
                urls: ice_connections.to_vec(),
                ..Default::default()
            }],
            ..Default::default()
        };

        let id = Uuid::new_v4();

        let connection = api.new_peer_connection(config).await?;
        let data_channel = connection
            .create_data_channel(&format!("data{id}"), None)
            .await?;

        Ok(Self {
            id: Uuid::new_v4(),
            connection,
            data_channel,
        })
    }

    pub(crate) async fn broadcast_offer(
        &self,
        signal_server_url: Url,
        room_config: RoomConfig,
    ) -> AResult<()> {
        let connection = &self.connection;

        let offer = connection.create_offer(None).await?;
        connection.set_local_description(offer).await?;

        let (tx, mut rx) = channel(2);
        connection.on_ice_candidate(Box::new(move |candidate| {
            let _ = tx.try_send(candidate);

            Box::pin(async {})
        }));

        let mut candidates = Vec::new();

        while let Some(candidate) = rx.recv().await {
            if let Some(candidate) = candidate {
                candidates.push(candidate);
            } else {
                break;
            }
        }

        let local_description = connection.local_description().await.ok_or(anyhow!(
            "Failed to get local description for the connection"
        ))?;

        let signal_server = SignalServer::new(signal_server_url, room_config);

        signal_server
            .broadcast_self(
                self.id.to_string(),
                &local_description,
                candidates.iter().map(|c| c.to_owned()).collect(),
            )
            .await?;

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use lazy_static::lazy_static;
    use webrtc::api::APIBuilder;

    lazy_static! {
        static ref ICE_SERVERS: Vec<String> = vec!["stun:stun.l.google.com:19302".to_string()];
    }

    #[tokio::test]
    async fn test_broadcast_offer() -> AResult<()> {
        let api = Arc::new(APIBuilder::new().build());

        let connection = P2PConnection::from_api(&ICE_SERVERS, api).await.unwrap();

        connection
            .broadcast_offer(
                Url::parse("http://localhost:8000").unwrap(),
                RoomConfig {
                    room: "my_room".into(),
                    channel: "my_channel".into(),
                },
            )
            .await?;

        Ok(())
    }
}
