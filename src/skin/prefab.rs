//! Note prefab system: maps skin entities to lane positions and sprite assignments.
//!
//! Creates note prototypes from skin entity definitions for each of the 7 lanes.

use crate::parsing::xml::{EntityDef, SkinDef};

/// Lane index (0-based, 7 lanes total).
pub const NUM_LANES: usize = 7;

/// A note prefab for a single lane.
#[derive(Debug, Clone)]
pub struct NotePrefab {
    pub lane: usize,
    pub x: i32,
    pub sprite_id: Option<String>,
    pub head_sprite: Option<String>,
    pub body_sprite: Option<String>,
    pub tail_sprite: Option<String>,
}

/// All note prefabs for a skin (one per lane).
#[derive(Debug, Clone)]
pub struct NotePrefabs {
    pub lanes: [NotePrefab; NUM_LANES],
    pub judgment_line_y: u32,
    pub skin_width: u32,
    pub skin_height: u32,
}

impl NotePrefabs {
    /// Create default 7-lane note prefabs when no skin XML is available.
    ///
    /// Distributes lanes evenly across the viewport width.
    pub fn default_7lan(skin_width: u32, skin_height: u32, judgment_line_y: u32) -> Self {
        let lane_width = skin_width as i32 / NUM_LANES as i32;
        let lanes: [NotePrefab; NUM_LANES] = std::array::from_fn(|lane| NotePrefab {
            lane,
            x: lane_width * lane as i32 + lane_width / 2,
            sprite_id: None,
            head_sprite: None,
            body_sprite: None,
            tail_sprite: None,
        });

        NotePrefabs {
            lanes,
            judgment_line_y,
            skin_width,
            skin_height,
        }
    }

    /// Build note prefabs from a parsed skin definition.
    ///
    /// Looks for entities with IDs like `NOTE_1` through `NOTE_7`
    /// and `LONG_NOTE_1` through `LONG_NOTE_7`.
    pub fn from_skin(skin: &SkinDef) -> Self {
        let mut lanes: [Option<NotePrefab>; NUM_LANES] = Default::default();

        for entity in &skin.entities {
            let lane_index = Self::extract_lane_from_note_entity(entity);
            if let Some(lane) = lane_index {
                if lane >= NUM_LANES {
                    continue;
                }

                lanes[lane].get_or_insert(NotePrefab {
                    lane,
                    x: entity.x,
                    sprite_id: entity.sprite.clone(),
                    head_sprite: None,
                    body_sprite: None,
                    tail_sprite: None,
                });
            }
        }

        // Fill in missing lanes with defaults
        for lane in 0..NUM_LANES {
            if lanes[lane].is_none() {
                lanes[lane] = Some(NotePrefab {
                    lane,
                    x: 0,
                    sprite_id: None,
                    head_sprite: None,
                    body_sprite: None,
                    tail_sprite: None,
                });
            }
        }

        // Unwrap is safe because we filled all lanes above
        let lanes: [NotePrefab; NUM_LANES] = lanes.map(|p| p.unwrap());

        NotePrefabs {
            lanes,
            judgment_line_y: skin.judgment_line_y,
            skin_width: skin.width,
            skin_height: skin.height,
        }
    }

    /// Extract the lane index from a note entity ID.
    ///
    /// Recognizes patterns like `NOTE_1` through `NOTE_7` and
    /// `LONG_NOTE_1` through `LONG_NOTE_7`.
    fn extract_lane_from_note_entity(entity: &EntityDef) -> Option<usize> {
        let id = entity.id.as_ref()?;

        // Try "NOTE_N" where N is 1-7
        if let Some(suffix) = id.strip_prefix("NOTE_") {
            if let Ok(n) = suffix.parse::<usize>() {
                if n >= 1 && n <= 7 {
                    return Some(n - 1); // Convert to 0-based
                }
            }
        }

        // Try "LONG_NOTE_N" where N is 1-7
        if let Some(suffix) = id.strip_prefix("LONG_NOTE_") {
            if let Ok(n) = suffix.parse::<usize>() {
                if n >= 1 && n <= 7 {
                    return Some(n - 1); // Convert to 0-based
                }
            }
        }

        None
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::xml::parse_str;
    use std::path::Path;

    const TEST_SKIN: &str = r#"<?xml version="1.0"?>
<Resources>
  <skin name="test" width="800" height="600" judgment_line="480">
    <layer>
      <frame id="note1" file="notes.png" x="0" y="0" w="32" h="25"/>
      <frame id="long_body" file="notes.png" x="64" y="0" w="32" h="8"/>
      <frame id="long_tail" file="notes.png" x="96" y="0" w="32" h="10"/>
      <entity id="NOTE_1" sprite="note1_sprite" x="100"/>
      <entity id="NOTE_2" sprite="note1_sprite" x="200"/>
      <entity id="NOTE_3" sprite="note1_sprite" x="300"/>
      <entity id="LONG_NOTE_1" head="note1_sprite" body="long_body" tail="long_tail" x="100"/>
      <entity id="LONG_NOTE_2" head="note1_sprite" body="long_body" tail="long_tail" x="200"/>
    </layer>
  </skin>
</Resources>"#;

    #[test]
    fn test_extract_lane_from_entity() {
        assert_eq!(NotePrefabs::extract_lane_from_note_entity(&EntityDef {
            id: Some("NOTE_1".to_string()),
            sprite: None, x: 0, y: 0, layer: 0,
        }), Some(0));

        assert_eq!(NotePrefabs::extract_lane_from_note_entity(&EntityDef {
            id: Some("NOTE_7".to_string()),
            sprite: None, x: 0, y: 0, layer: 0,
        }), Some(6));

        assert_eq!(NotePrefabs::extract_lane_from_note_entity(&EntityDef {
            id: Some("LONG_NOTE_3".to_string()),
            sprite: None, x: 0, y: 0, layer: 0,
        }), Some(2));

        // Non-note entities return None
        assert_eq!(NotePrefabs::extract_lane_from_note_entity(&EntityDef {
            id: Some("JUDGMENT_LINE".to_string()),
            sprite: None, x: 0, y: 0, layer: 0,
        }), None);
    }

    #[test]
    fn test_build_prefabs_from_skin() {
        let resources = parse_str(TEST_SKIN, Path::new("")).expect("Failed to parse");
        let skin = resources.get_skin("test").expect("Skin not found");
        let prefabs = NotePrefabs::from_skin(skin);

        assert_eq!(prefabs.judgment_line_y, 480);
        assert_eq!(prefabs.skin_width, 800);
        assert_eq!(prefabs.skin_height, 600);

        // Lane 0 should have NOTE_1 entity
        let lane0 = &prefabs.lanes[0];
        assert_eq!(lane0.lane, 0);
        assert_eq!(lane0.x, 100);
        assert_eq!(lane0.sprite_id.as_deref(), Some("note1_sprite"));

        // Lane 1 should have NOTE_2
        let lane1 = &prefabs.lanes[1];
        assert_eq!(lane1.x, 200);

        // Lane 2 should have NOTE_3
        let lane2 = &prefabs.lanes[2];
        assert_eq!(lane2.x, 300);

        // Lanes 3-6 should have defaults
        for lane in 3..NUM_LANES {
            assert_eq!(prefabs.lanes[lane].x, 0);
            assert!(prefabs.lanes[lane].sprite_id.is_none());
        }
    }

    #[test]
    fn test_prefabs_has_all_lanes() {
        let resources = parse_str(TEST_SKIN, Path::new("")).expect("Failed to parse");
        let skin = resources.get_skin("test").expect("Skin not found");
        let prefabs = NotePrefabs::from_skin(skin);

        // All 7 lanes should exist
        for lane in 0..NUM_LANES {
            assert_eq!(prefabs.lanes[lane].lane, lane);
        }
    }

    #[test]
    fn test_invalid_lane_numbers() {
        // NOTE_8 should be ignored (only 1-7 are valid)
        let entity = EntityDef {
            id: Some("NOTE_8".to_string()),
            sprite: None, x: 0, y: 0, layer: 0,
        };
        assert!(NotePrefabs::extract_lane_from_note_entity(&entity).is_none());

        // NOTE_0 should be ignored
        let entity = EntityDef {
            id: Some("NOTE_0".to_string()),
            sprite: None, x: 0, y: 0, layer: 0,
        };
        assert!(NotePrefabs::extract_lane_from_note_entity(&entity).is_none());
    }
}
