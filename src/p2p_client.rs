use crate::p2p_connection::P2PConnection;
use std::{collections::HashMap, fmt::Debug};
use uuid::Uuid;
use webrtc::api::{APIBuilder, API};

pub(crate) trait IntoId: Debug {
    fn id(&self) -> String;
}

impl IntoId for Uuid {
    fn id(&self) -> String {
        self.to_string()
    }
}

impl IntoId for String {
    fn id(&self) -> String {
        self.clone()
    }
}

/// A wrapper around the webrtc connections.
/// Has a `Default` impl which passes stun:stun.l.google.com:19302 to the `P2PClient::new`
/// constructor
pub struct P2PClient<'a> {
    pub(crate) id: Box<dyn IntoId>,
    pub(crate) api: API,
    connections: HashMap<String, P2PConnection<'a>>,
    pub(crate) ice_servers: Vec<String>,
}

impl<'a> P2PClient<'a> {
    pub fn new(ice_servers: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let servers = ice_servers
            .into_iter()
            .map(|s| s.into())
            .collect::<Vec<String>>();

        let api = APIBuilder::new().build();

        Self {
            ice_servers: servers,
            id: Box::new(Uuid::new_v4()),
            connections: Default::default(),
            api,
        }
    }
}

impl<'a> Default for P2PClient<'a> {
    fn default() -> Self {
        Self::new(["stun:stun.l.google.com:19302"])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULT_SERVER: &str = "stun:stun.l.google.com:19302";

    #[test]
    fn test_new_connections() -> anyhow::Result<()> {
        let server = "stun:stun.l.google.com:19302";
        let client = P2PClient::new([server]);

        assert_eq!(client.ice_servers[0], server);
        Ok(())
    }

    #[test]
    fn test_default() -> anyhow::Result<()> {
        let client = P2PClient::default();

        assert_eq!(client.ice_servers[0], DEFAULT_SERVER);
        Ok(())
    }
}
