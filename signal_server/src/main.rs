#[macro_use]
extern crate rocket;
use rocket::{
    response::status::{BadRequest, NotFound},
    serde::json::Json,
    tokio::sync::RwLock,
    State,
};
use serde::{Deserialize, Serialize};
use signal_server::BroadcastCandidateArgs;
use std::{collections::HashMap, sync::Arc};
use uuid::Uuid;
use webrtc::{
    ice_transport::ice_candidate::RTCIceCandidate,
    peer_connection::sdp::session_description::RTCSessionDescription,
};

fn get_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[derive(Debug)]
struct IceCandidateWithInitTime {
    candidate: Vec<RTCIceCandidate>,
    session_description: Option<RTCSessionDescription>,
    init_time: u64,
}

impl Default for IceCandidateWithInitTime {
    fn default() -> Self {
        Self {
            session_description: None,
            candidate: Vec::new(),
            init_time: get_now(),
        }
    }
}

struct SocketRooms(HashMap<String, HashMap<Uuid, IceCandidateWithInitTime>>);

struct SocketChannels(HashMap<String, SocketRooms>);

type RoomMap = Arc<RwLock<SocketChannels>>;

#[derive(Serialize, Deserialize)]
struct RoomCandidate {
    candidate_id: String,
    candidate: RTCIceCandidate,
}

#[get("/candidate?<channel>&<room>&<candidate_id>")]
async fn get_room_candidate(
    room_map_state: &State<RoomMap>,
    channel: String,
    room: String,
    candidate_id: String,
) -> Result<Json<Vec<RTCIceCandidate>>, NotFound<()>> {
    let candidate_uuid = Uuid::parse_str(candidate_id.as_str()).map_err(|_| NotFound(()))?;

    let room_map = room_map_state.read().await;
    let rooms = room_map.0.get(channel.as_str()).ok_or(NotFound(()))?;
    let room = rooms.0.get(room.as_str()).ok_or(NotFound(()))?;
    let candidate = room.get(&candidate_uuid).ok_or(NotFound(()))?;

    Ok(Json(candidate.candidate.clone()))
}

#[get("/all_candidates?<channel>&<room>")]
async fn get_candidates_in_room(
    room_map_state: &State<RoomMap>,
    channel: String,
    room: String,
) -> Result<Json<String>, NotFound<()>> {
    let room_map = room_map_state.read().await;
    let rooms = room_map.0.get(channel.as_str()).ok_or(NotFound(()))?;
    let room = rooms.0.get(room.as_str()).ok_or(NotFound(()))?;

    Ok(Json(room.keys().map(|v| v.to_string()).collect()))
}

#[get("/rooms?<channel>")]
async fn get_rooms(
    room_map_state: &State<RoomMap>,
    channel: String,
) -> Result<Json<Vec<String>>, NotFound<()>> {
    let room_map = room_map_state.read().await;
    let rooms = &room_map.0.get(channel.as_str()).ok_or(NotFound(()))?.0;

    Ok(Json(rooms.keys().map(|uuid| uuid.to_string()).collect()))
}

#[post(
    "/announce?<channel>&<room>&<peer_id>",
    format = "json",
    data = "<candidate_args>"
)]
async fn broadcast_candidate(
    channel: String,
    room: String,
    peer_id: String,
    candidate_args: Json<BroadcastCandidateArgs>,
    room_map_state: &State<RoomMap>,
) -> Result<(), BadRequest<()>> {
    let mut room_map = room_map_state.write().await;

    let channel_entry = room_map
        .0
        .entry(channel)
        .or_insert_with(|| SocketRooms(HashMap::new()));

    let room_entry = channel_entry
        .0
        .entry(room)
        .or_insert_with(|| HashMap::new());

    let candidate = IceCandidateWithInitTime {
        candidate: candidate_args.candidates.clone(),
        init_time: get_now(),
        session_description: candidate_args.session_description.clone(),
    };

    let uuid = Uuid::parse_str(peer_id.as_str()).map_err(|_| BadRequest(()))?;

    let entry = room_entry
        .entry(uuid)
        .or_insert(IceCandidateWithInitTime::default());
    entry.candidate.extend(candidate.candidate);
    entry.session_description = candidate.session_description;

    println!("{entry:?}");

    Ok(())
}

#[launch]
async fn rocket() -> _ {
    let room_map_state: RoomMap = Arc::new(RwLock::new(SocketChannels(HashMap::new())));

    let cloned_room_state = room_map_state.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
            let mut room_map = cloned_room_state.write().await;

            for (_, rooms) in room_map.0.iter_mut() {
                for (_, room) in rooms.0.iter_mut() {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs();

                    room.retain(|_, v| now - v.init_time < 60);
                }
            }

            // Filter the rooms that have no candidates
            room_map.0.retain(|_, v| !v.0.is_empty());
        }
    });

    rocket::build().manage(room_map_state).mount(
        "/",
        routes![
            get_candidates_in_room,
            get_room_candidate,
            get_rooms,
            broadcast_candidate
        ],
    )
}
