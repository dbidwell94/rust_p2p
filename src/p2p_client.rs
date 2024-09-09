use crate::{p2p_connection::P2PConnection, signal_server::RoomConfig};
use anyhow::{anyhow, Result as AResult};
use reqwest::Url;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, RwLock,
    },
    u16,
};
use tokio::task::JoinHandle;
use webrtc::api::APIBuilder;

pub struct P2PClient {
    ice_servers: Vec<String>,
    max_connections: u16,
    has_pending_connection: Arc<AtomicBool>,
    active_connections: Arc<RwLock<Vec<P2PConnection>>>,
    connection_listener_handle: Option<JoinHandle<AResult<()>>>,
    rtc_api: Arc<webrtc::api::API>,
    signal_server_url: Url,
}

impl P2PClient {
    /// Accept a single connection from the signaling server
    pub fn accept_connections(&mut self, room_config: RoomConfig) -> AResult<()> {
        let ice_servers = self.ice_servers.clone();
        let api = self.rtc_api.clone();
        let has_pending_connection = self.has_pending_connection.clone();
        let active_connections = self.active_connections.clone();
        let max_connections = self.max_connections.clone();
        let signal_server_url = self.signal_server_url.clone();
        let handle: JoinHandle<AResult<()>> = tokio::spawn(async move {
            loop {
                let connection = P2PConnection::from_api(&ice_servers, api.clone()).await?;
                has_pending_connection.store(true, Ordering::Relaxed);

                connection
                    .broadcast_offer(signal_server_url.clone(), room_config.clone())
                    .await?;

                let active_connection_count;
                // Aquire the write lock and immediately release
                {
                    let mut active_connections = active_connections.write().map_err(|_| {
                        anyhow!("Unable to aquire write lock for the active connections")
                    })?;

                    active_connections.push(connection);
                    active_connection_count = active_connections.len();
                }

                // check to ensure the active connection count is less than the max connections, or else break
                if active_connection_count >= max_connections as usize {
                    break;
                }
            }

            Ok(())
        });
        self.connection_listener_handle = Some(handle);

        Ok(())
    }

    pub fn has_pending_connection(&self) -> AResult<bool> {
        let has_connections = self.has_pending_connection.load(Ordering::Relaxed);

        Ok(has_connections)
    }

    pub fn active_connections_count(&self) -> AResult<usize> {
        Ok(self
            .active_connections
            .read()
            .map_err(|_| anyhow!("Unable to aquire read lock"))?
            .len())
    }
}

pub struct P2PClientBuilder {
    ice_servers: Vec<String>,
    max_connections: u16,
    signal_server_url: Url,
}

impl P2PClientBuilder {
    pub fn new(signal_server_url: Url) -> Self {
        Self {
            ice_servers: Vec::new(),
            max_connections: u16::MAX,
            signal_server_url,
        }
    }

    pub fn with_ice_servers(mut self, ice_servers: Vec<String>) -> Self {
        self.ice_servers = ice_servers;
        self
    }

    pub fn with_max_connections(mut self, max_connections: u16) -> Self {
        self.max_connections = max_connections;
        self
    }

    pub fn build(self) -> P2PClient {
        let api = Arc::new(APIBuilder::new().build());

        P2PClient {
            ice_servers: self.ice_servers,
            max_connections: self.max_connections,
            has_pending_connection: Arc::new(AtomicBool::new(false)),
            active_connections: Arc::new(RwLock::new(Vec::new())),
            connection_listener_handle: None,
            rtc_api: api,
            signal_server_url: self.signal_server_url,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    lazy_static::lazy_static!(
        static ref SIGNAL_SERVER_URL: Url = "http://localhost:8000".try_into().unwrap();
    );

    #[test]
    fn test_builder() {
        let client = P2PClientBuilder::new(SIGNAL_SERVER_URL.clone())
            .with_ice_servers(vec!["stun:stun.l.google.com:19302".to_string()])
            .with_max_connections(10)
            .build();

        assert_eq!(
            client.ice_servers,
            vec!["stun:stun.l.google.com:19302".to_string()]
        );
        assert_eq!(client.max_connections, 10);
    }
}
