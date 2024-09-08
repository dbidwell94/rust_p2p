use reqwest::{Client, Url};
use thiserror::Error;
use uuid::Uuid;
use webrtc::ice_transport::ice_candidate::RTCIceCandidate;

#[derive(Error, Debug)]
pub enum SignalServerError {
    #[error(transparent)]
    ApiError(#[from] reqwest::Error),
    #[error("An unknown error has occurred")]
    GeneralError,
}

pub struct SignalServerClient {
    api_client: Client,
    base_url: Url,
}

impl SignalServerClient {
    pub fn new(server_url: Url) -> Self {
        Self {
            api_client: Client::new(),
            base_url: server_url,
        }
    }

    pub async fn announce_self(
        &self,
        channel: &str,
        room: &str,
        p2p_id: &str,
        ice_candidate: &[RTCIceCandidate],
    ) -> Result<(), SignalServerError> {
        let url = self
            .base_url
            .join(&format!("/announce"))
            .map_err(|_| SignalServerError::GeneralError)?;

        let query = vec![("channel", channel), ("room", room), ("token", p2p_id)];

        self.api_client
            .post(url)
            .query(&query)
            .json(ice_candidate)
            .send()
            .await?;

        Ok(())
    }

    pub async fn get_candidates_in_room(
        &self,
        channel: &str,
        room: &str,
    ) -> Result<Vec<Uuid>, SignalServerError> {
        let url = self
            .base_url
            .join(&format!("/allCandidates"))
            .map_err(|_| SignalServerError::GeneralError)?;

        let query = vec![("channel", channel), ("room", room)];

        let response = self
            .api_client
            .get(url)
            .query(&query)
            .send()
            .await?
            .json::<Vec<String>>()
            .await?
            .iter()
            .flat_map(|v| Uuid::parse_str(v))
            .collect::<Vec<_>>();

        Ok(response)
    }
}
