use crate::{p2p_connection::RoomInfo, signal_server::SignalServerClient};

use super::p2p_connection::P2PConnection;
use anyhow::Result as AResult;
use reqwest::Url;
use std::sync::Arc;
use tokio::{
    select,
    sync::mpsc::{channel, Receiver},
    task::JoinHandle,
};
pub use tokio_util::sync::CancellationToken;
use webrtc::{
    api::{APIBuilder, API},
    ice_transport::ice_server::RTCIceServer,
    peer_connection::configuration::RTCConfiguration,
};

pub struct P2PClient {
    api: Arc<API>,
    connection_listen_handle: Option<JoinHandle<AResult<()>>>,
    connection_cancellation_token: Option<CancellationToken>,
    signal_server_client: Arc<SignalServerClient>,
    room_info: RoomInfo,
}

impl P2PClient {
    pub fn new(signal_server_url: &Url, room_info: RoomInfo) -> Self {
        let api_url: Url = signal_server_url.clone();

        let api = APIBuilder::new().build();
        Self {
            api: Arc::new(api),
            connection_listen_handle: None,
            connection_cancellation_token: None,
            signal_server_client: Arc::new(SignalServerClient::new(api_url)),
            room_info,
        }
    }

    pub fn finish_listen_for_connections(&mut self) {
        if let Some(cancellation_token) = self.connection_cancellation_token.take() {
            cancellation_token.cancel();
        }

        if let Some(handle) = self.connection_listen_handle.take() {
            handle.abort();
        }
    }

    pub fn listen_for_connections(
        &mut self,
        ice_servers: Vec<String>,
        cancellation_token: CancellationToken,
    ) -> AResult<Receiver<P2PConnection>> {
        let api = self.api.clone();
        let cancellation_token_to_store = cancellation_token.clone();

        let (tx, rx) = channel(1);

        let cloned_signal_server_client = self.signal_server_client.clone();
        let room_info = self.room_info.clone();
        let join_handle: JoinHandle<AResult<()>> = tokio::task::spawn(async move {
            loop {
                let child_token = cancellation_token.child_token();
                let peer_connection = api
                    .new_peer_connection(RTCConfiguration {
                        ice_servers: vec![RTCIceServer {
                            urls: ice_servers.clone(),
                            ..Default::default()
                        }],
                        ..Default::default()
                    })
                    .await?;

                select! {
                    _ = child_token.cancelled() => {
                        break;
                    }
                    connection = P2PConnection::from_peer_connection(peer_connection, cloned_signal_server_client.clone(), room_info.clone()) => {
                        tx.send(connection?).await?;
                    }
                };
            }

            Ok(())
        });

        self.connection_listen_handle = Some(join_handle);
        self.connection_cancellation_token = Some(cancellation_token_to_store);

        Ok(rx)
    }
}

impl Drop for P2PClient {
    fn drop(&mut self) {
        if let Some(cancellation_token) = self.connection_cancellation_token.take() {
            cancellation_token.cancel();
        }

        if let Some(handle) = self.connection_listen_handle.take() {
            handle.abort();
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    lazy_static::lazy_static! {
        static ref SIGNAL_SERVER_URL: Url = Url::parse("http://localhost:8000").unwrap();
    }

    #[tokio::test]
    async fn test_p2p_client() -> AResult<()> {
        let mut client = P2PClient::new(
            &SIGNAL_SERVER_URL,
            RoomInfo {
                channel: "test".to_string(),
                room: "test".to_string(),
            },
        );

        let ice_servers = vec!["stun:stun.l.google.com:19302".to_string()];

        let cancellation_token = CancellationToken::new();

        let mut connection_receiver =
            client.listen_for_connections(ice_servers, cancellation_token.child_token())?;

        // tokio::time::sleep(Duration::from_secs(1)).await;

        // cancellation_token.cancel();

        while let Some(_) = connection_receiver.recv().await {
            println!("Received connection");
        }

        Ok(())
    }
}
