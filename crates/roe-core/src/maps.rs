//! The game's maps and what the community server can do with them. The map is `profile.base.m644` in the
//! server's recorded enter-room reply (applied by `playername::apply`). The IDs are the match ("PLANE") maps of the
//! game's own map table (an XML in `SFC/TableBin.sfc`, `m_vecMapConfig`): 16010 Dione select-point start,
//! 16011 Dione night mode, 16007 island (Kill Zone) select-point start, 16021..16024 Desert ("Gobi"; 16022 novice,
//! 16023 practice). Only Dione has server content (spawn points, loot, zones, evacuation are hand-built for 16010).
//!
//! RoE-Rivival (`RIVIVAL_MAPS`) hosts more maps itself and takes the map as `--map ID`, with its own IDs
//! (`server/directory/map_profiles_2022.py`): 16014 Europa (the client's 16007, the Kill Zone world), 16023 Vera
//! (the client's 16024, the desert world), 16011 Dione at night, 16903 Europa Season 3.

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
               appears outside the final zone and too high to reach. You can't pick a landing point, and the starting \
               packs are Dione's (a snowboard instead of the BMX, which slides on grass and sand too). For testing only.",
    },
    Map {
        name: "Desert",
        world: "EU_Desert_4096",
        id: Some(16021),
        support: Support::Broken,
        badge: "Falls through the ground",
        note: "Loads, but the landing-point picker doesn't respond; at the start you appear high above the map and \
               fall through the ground without end, and the server then refuses a new match until it restarts. The \
               server only knows Dione's terrain (Desert is lower and smaller).",
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

/// livegaming2042's RoE-Rivival (kit of 2026-10-29): Dione, Europa and Vera in every mode, by its author.
pub const RIVIVAL_MAPS: [Map; 5] = [
    Map {
        name: "Dione",
        world: "EU_IceLand_4096",
        id: Some(DIONE),
        support: Support::Supported,
        badge: "All modes",
        note: "Play, Training and Customs. In Customs, pick the same map as here: the server hosts one map per session.",
    },
    Map {
        name: "Europa",
        world: "EU_Killzone_4096",
        id: Some(16014),
        support: Support::Supported,
        badge: "All modes",
        note: "Play, Training and Customs. In Customs, pick the same map as here: the server hosts one map per session.",
    },
    Map {
        name: "Vera",
        world: "EU_Desert_4096",
        id: Some(16023),
        support: Support::Supported,
        badge: "All modes",
        note: "Play, Training and Customs. In Customs, pick the same map as here: the server hosts one map per session.",
    },
    Map {
        name: "Dione at night",
        world: "EU_IcelandNight_4096",
        id: Some(16011),
        support: Support::Experimental,
        badge: "Untested by its author",
        note: "The server has it, but its author hasn't tested it yet. No storm and no helicopter.",
    },
    Map {
        name: "Europa Season 3",
        world: "Season 3 patch",
        id: Some(16903),
        support: Support::Broken,
        badge: "Needs a client patch",
        note: "Needs a patch that changes the game's files, which the launcher doesn't apply.",
    },
];

/// The map with this ID in `maps`, if it is one the launcher may select.
pub fn by_id(maps: &'static [Map], id: u32) -> Option<&'static Map> {
    maps.iter().find(|m| m.id == Some(id) && m.selectable())
}

impl Map {
    pub fn selectable(&self) -> bool {
        matches!(self.support, Support::Supported | Support::Experimental)
    }
}

/// The ID to start the server with: a saved choice that isn't selectable (any more) falls back to Dione.
pub fn playable_id(maps: &'static [Map], id: u32) -> u32 {
    by_id(maps, id).map_or(DIONE, |m| m.id.unwrap_or(DIONE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_known_maps_can_be_chosen() {
        assert_eq!(playable_id(&MAPS, DIONE), DIONE);
        assert_eq!(playable_id(&MAPS, 16007), 16007);
        assert_eq!(by_id(&MAPS, 16007).unwrap().support, Support::Experimental);
        // night hangs the game and desert drops you through the ground: a saved choice falls back to Dione
        assert_eq!(playable_id(&MAPS, 16011), DIONE);
        assert_eq!(playable_id(&MAPS, 16021), DIONE);
        // the dev test map, or anything else, falls back to Dione
        assert_eq!(playable_id(&MAPS, 16005), DIONE);
        assert_eq!(playable_id(&MAPS, 0), DIONE);
        assert!(MAPS.iter().filter(|m| m.support == Support::Supported).all(|m| m.id == Some(DIONE)));
    }

    #[test]
    fn rivival_hosts_its_own_maps() {
        assert_eq!(playable_id(&RIVIVAL_MAPS, 16014), 16014);
        assert_eq!(playable_id(&RIVIVAL_MAPS, 16023), 16023);
        assert_eq!(playable_id(&RIVIVAL_MAPS, 16011), 16011);
        // Season 3 needs a client patch; ROE-LIVE's Kill Zone ID is not one of its maps
        assert_eq!(playable_id(&RIVIVAL_MAPS, 16903), DIONE);
        assert_eq!(playable_id(&RIVIVAL_MAPS, 16007), DIONE);
        assert_eq!(RIVIVAL_MAPS[0].id, Some(DIONE));
    }
}
