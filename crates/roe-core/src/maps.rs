//! The game's maps and what the community server can do with them. The map is `profile.base.m644` in the
//! server's recorded enter-room reply (applied by `playername::apply`). The IDs are the match ("PLANE") maps of the
//! game's own map table (an XML in `SFC/TableBin.sfc`, `m_vecMapConfig`): 16010 Dione select-point start,
//! 16011 Dione night mode, 16007 island (Kill Zone) select-point start, 16021..16024 Desert ("Gobi"; 16022 novice,
//! 16023 practice). Only Dione has server content (spawn points, loot, zones, evacuation are hand-built for 16010).

/// Upstream's map: Dione (winter).
pub const DIONE: u32 = 16010;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Support {
    /// Playable in Training on the community server.
    Supported,
    /// The game has it and its ID is known, but the server only has Dione's content: spawns, loot and zones will
    /// be Dione's. Selectable, at the player's risk.
    Experimental,
    /// Tested and unplayable (e.g. the game hangs): shown, but can't be picked.
    Broken,
}

#[derive(Clone, Copy, Debug)]
pub struct Map {
    pub name: &'static str,
    /// Folder under the game's `Cache/Data/world/`.
    pub world: &'static str,
    pub id: Option<u32>,
    pub support: Support,
    /// Short status on the tile.
    pub badge: &'static str,
    /// What we know from testing (tooltip).
    pub note: &'static str,
}

/// Ordered from working to most broken (owner's tests, 2026-10-19).
pub const MAPS: [Map; 4] = [
    Map {
        name: "Dione",
        world: "EU_IceLand_4096",
        id: Some(DIONE),
        support: Support::Supported,
        badge: "Training",
        note: "Playable in Training on the community server.",
    },
    Map {
        name: "Kill Zone",
        world: "EU_Killzone_4096",
        id: Some(16007),
        support: Support::Experimental,
        badge: "Plays · no extraction",
        note: "Loads and plays (moving, swimming), but the server still uses Dione's zones and evacuation: the helicopter \
               appears outside the final zone and too high to reach. For testing only.",
    },
    Map {
        name: "Desert",
        world: "EU_Desert_4096",
        id: Some(16021),
        support: Support::Experimental,
        badge: "Untested",
        note: "Not tried yet. The server only has Dione's spawn points, loot, zones and evacuation, and Desert is a \
               smaller map, so expect odd spawns, missing loot or no way to extract.",
    },
    Map {
        name: "Dione at night",
        world: "EU_IcelandNight_4096",
        id: Some(16011),
        support: Support::Broken,
        badge: "Hangs at match start",
        note: "The night event map: the landing-point picker doesn't respond and the game freezes when the countdown \
               ends. Needs server support for its event mode.",
    },
];

/// The map with this ID, if it is one the launcher may select.
pub fn by_id(id: u32) -> Option<&'static Map> {
    MAPS.iter().find(|m| m.id == Some(id) && m.selectable())
}

impl Map {
    pub fn selectable(&self) -> bool {
        matches!(self.support, Support::Supported | Support::Experimental)
    }
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
        assert_eq!(playable_id(16021), 16021);
        // night hangs the game: a saved choice falls back to Dione
        assert_eq!(playable_id(16011), DIONE);
        // the dev test map, or anything else, falls back to Dione
        assert_eq!(playable_id(16005), DIONE);
        assert_eq!(playable_id(0), DIONE);
        assert!(MAPS.iter().filter(|m| m.support == Support::Supported).all(|m| m.id == Some(DIONE)));
    }
}
