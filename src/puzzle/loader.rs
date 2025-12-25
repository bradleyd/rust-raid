use anyhow::Result;
use std::path::Path;

use super::types::Room;

pub fn load_puzzle(path: &Path) -> Result<Room> {
    let content = std::fs::read_to_string(path)?;
    let room: Room = toml::from_str(&content)?;
    Ok(room)
}

pub fn load_floor(floor_dir: &Path) -> Result<Vec<Room>> {
    let mut rooms = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(floor_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.starts_with("room_") && name.ends_with(".toml")
        })
        .collect();

    // Sort by filename so room_01, room_02, room_03 are in order
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let room = load_puzzle(&entry.path())?;
        rooms.push(room);
    }

    Ok(rooms)
}
