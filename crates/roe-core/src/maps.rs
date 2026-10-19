//! The game's maps and what the community server can do with them. The map is `profile.base.m644` in the
//! server's recorded enter-room reply (applied by `playername::apply`). IDs come from ROE-LIVE's research notes
//! (docs/BUILDS.md, "Steam table"); the world folders are the game's `Cache/Data/world/*`.
//! Only Dione has server content (spawn points, loot, zones, evacuation are all hand-built for map 16010).

/// Upstream's map: Dione (winter).
pub const DIONE: u32 = 16010;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Support {
    /// Playable in Training on the community server.
    Supported,
    /// The game has it and its ID is known, but the server only has Dione's content: spawns, loot and zones will
    /// be Dione's. Selectable, at the player's risk.
    Experimental,
    /// The game has it, but its map ID isn't known yet.
    Unknown,
}

#[derive(Clone, Copy, Debug)]
pub struct Map {
    pub name: &'static str,
    /// Folder under the game's `Cache/Data/world/`.
    pub world: &'static str,
    pub id: Option<u32>,
    pub support: Support,
}

pub const MAPS: [Map; 4] = [
    Map { name: "Dione", world: "EU_IceLand_4096", id: Some(DIONE), support: Support::Supported },
    Map { name: "Dione at night", world: "EU_IcelandNight_4096", id: None, support: Support::Unknown },
    Map { name: "Desert", world: "EU_Desert_4096", id: None, support: Support::Unknown },
    Map { name: "Kill Zone", world: "EU_Killzone_4096", id: Some(16007), support: Support::Experimental },
];

/// The map with this ID, if it is one the launcher may select.
pub fn by_id(id: u32) -> Option<&'static Map> {
    MAPS.iter().find(|m| m.id == Some(id) && m.support != Support::Unknown)
}

/// The ID to put in the room reply: a saved choice that isn't selectable (any more) falls back to Dione.
pub fn playable_id(id: u32) -> u32 {
    by_id(id).map_or(DIONE, |m| m.id.unwrap_or(DIONE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_known_maps_can_be_chosen() {
        assert_eq!(playable_id(DIONE), DIONE);
        assert_eq!(playable_id(16007), 16007);
        assert_eq!(by_id(16007).unwrap().support, Support::Experimental);
        // the dev test map, or anything else, falls back to Dione
        assert_eq!(playable_id(16005), DIONE);
        assert_eq!(playable_id(0), DIONE);
        assert!(MAPS.iter().filter(|m| m.support == Support::Supported).all(|m| m.id == Some(DIONE)));
    }
}
