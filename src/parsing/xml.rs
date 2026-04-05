//! Skin XML parser using quick-xml.
//!
//! Parses `<Resources>` XML files defining skins, sprites, frames, and entities.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use quick_xml::events::Event;
use quick_xml::Reader;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A rectangular frame sliced from a spritesheet image.
#[derive(Debug, Clone)]
pub struct FrameDef {
    pub id: String,
    pub file: PathBuf,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub scale: f32,
    pub alpha: f32,
}

/// An animated sprite composed of frame references.
#[derive(Debug, Clone)]
pub struct SpriteDef {
    pub id: String,
    pub frame_refs: Vec<String>,
    pub frame_speed_ms: u32,
    pub loop_animation: bool,
}

/// A game entity definition from the skin XML.
#[derive(Debug, Clone)]
pub struct EntityDef {
    pub id: String,
    pub sprite: Option<String>,
    pub x: i32,
    pub y: i32,
    pub head_sprite: Option<String>,
    pub body_sprite: Option<String>,
    pub tail_sprite: Option<String>,
    pub layer: u32,
}

/// A complete skin definition from the XML.
#[derive(Debug, Clone)]
pub struct SkinDef {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub judgment_line_y: u32,
    pub frames: Vec<FrameDef>,
    pub sprites: Vec<SpriteDef>,
    pub entities: Vec<EntityDef>,
}

/// All skins parsed from a resources.xml file.
#[derive(Debug, Clone)]
pub struct Resources {
    pub skins: HashMap<String, SkinDef>,
    pub base_path: PathBuf,
}

impl Resources {
    pub fn get_skin(&self, name: &str) -> Option<&SkinDef> {
        self.skins.get(name)
    }

    pub fn skin_names(&self) -> Vec<&String> {
        self.skins.keys().collect()
    }
}

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum XmlError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("XML parse error: {0}")]
    Parse(String),
    #[error("missing required attribute: {0}")]
    MissingAttribute(String),
    #[error("invalid attribute value: {attr}={value}")]
    InvalidAttributeValue { attr: String, value: String },
}

// ---------------------------------------------------------------------------
// Attribute parsing helpers
// ---------------------------------------------------------------------------

fn get_attr<'a>(attrs: &'a [quick_xml::events::attributes::Attribute<'a>], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|a| a.key.as_ref() == name.as_bytes())
        .and_then(|a| std::str::from_utf8(&a.value).ok())
}

fn get_attr_required<'a>(attrs: &'a [quick_xml::events::attributes::Attribute<'a>], name: &str) -> Result<&'a str, XmlError> {
    get_attr(attrs, name).ok_or_else(|| XmlError::MissingAttribute(name.to_string()))
}

fn parse_attr_u32(attrs: &[quick_xml::events::attributes::Attribute<'_>], name: &str) -> Result<u32, XmlError> {
    let val = get_attr_required(attrs, name)?;
    val.parse().map_err(|_| XmlError::InvalidAttributeValue {
        attr: name.to_string(),
        value: val.to_string(),
    })
}

fn parse_attr_u32_default(attrs: &[quick_xml::events::attributes::Attribute<'_>], name: &str, default: u32) -> u32 {
    match get_attr(attrs, name) {
        Some(val) => val.parse().unwrap_or(default),
        None => default,
    }
}

fn parse_attr_f32(attrs: &[quick_xml::events::attributes::Attribute<'_>], name: &str, default: f32) -> Result<f32, XmlError> {
    match get_attr(attrs, name) {
        Some(val) => val.parse().map_err(|_| XmlError::InvalidAttributeValue {
            attr: name.to_string(),
            value: val.to_string(),
        }),
        None => Ok(default),
    }
}

fn parse_attr_i32(attrs: &[quick_xml::events::attributes::Attribute<'_>], name: &str, default: i32) -> Result<i32, XmlError> {
    match get_attr(attrs, name) {
        Some(val) => val.parse().map_err(|_| XmlError::InvalidAttributeValue {
            attr: name.to_string(),
            value: val.to_string(),
        }),
        None => Ok(default),
    }
}

fn parse_attr_bool(attrs: &[quick_xml::events::attributes::Attribute<'_>], name: &str, default: bool) -> bool {
    match get_attr(attrs, name) {
        Some(val) => matches!(val.to_lowercase().as_str(), "true" | "1" | "yes"),
        None => default,
    }
}

/// Collect attributes from a BytesStart event into a Vec for repeated access.
fn collect_attrs<'a>(e: &'a quick_xml::events::BytesStart<'a>) -> Result<Vec<quick_xml::events::attributes::Attribute<'a>>, XmlError> {
    e.attributes().collect::<Result<Vec<_>, _>>()
        .map_err(|err| XmlError::Parse(err.to_string()))
}

/// Get the tag name as a String from a BytesStart event.
fn tag_name(e: &quick_xml::events::BytesStart<'_>) -> Result<String, XmlError> {
    std::str::from_utf8(e.name().as_ref())
        .map_err(|err| XmlError::Parse(err.to_string()))
        .map(String::from)
}

/// Get the tag name as a String from a BytesEnd event.
fn tag_name_end(e: &quick_xml::events::BytesEnd<'_>) -> Result<String, XmlError> {
    std::str::from_utf8(e.name().as_ref())
        .map_err(|err| XmlError::Parse(err.to_string()))
        .map(String::from)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parse a resources.xml file from disk.
pub fn parse_file(path: impl AsRef<Path>) -> Result<Resources, XmlError> {
    let path = path.as_ref();
    let data = std::fs::read_to_string(path)?;
    let base_path = path.parent().unwrap_or(Path::new("")).to_path_buf();
    parse_str(&data, &base_path)
}

/// Parse skin XML from a string.
pub fn parse_str(xml: &str, base_path: &Path) -> Result<Resources, XmlError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut skins: HashMap<String, SkinDef> = HashMap::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                if tag_name(e)? == "skin" {
                    let skin = parse_skin(&mut reader, e, base_path)?;
                    skins.insert(skin.name.clone(), skin);
                }
            }
            Ok(Event::Eof) => break,
            Err(err) => return Err(XmlError::Parse(err.to_string())),
            _ => {}
        }
        buf.clear();
    }

    Ok(Resources {
        skins,
        base_path: base_path.to_path_buf(),
    })
}

// ---------------------------------------------------------------------------
// Skin parsing
// ---------------------------------------------------------------------------

fn parse_skin<R: std::io::BufRead>(
    reader: &mut Reader<R>,
    skin_event: &quick_xml::events::BytesStart<'_>,
    base_path: &Path,
) -> Result<SkinDef, XmlError> {
    let attrs = collect_attrs(skin_event)?;

    let name = get_attr_required(&attrs, "name")?.to_string();
    let width = parse_attr_u32(&attrs, "width")?;
    let height = parse_attr_u32(&attrs, "height")?;
    let judgment_line_y = parse_attr_u32(&attrs, "judgment_line")?;

    let mut frames: Vec<FrameDef> = Vec::new();
    let mut sprites: Vec<SpriteDef> = Vec::new();
    let mut entities: Vec<EntityDef> = Vec::new();
    let mut current_layer: u32 = 0;
    let mut frame_buffer: Vec<FrameDef> = Vec::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag = tag_name(e)?;
                match tag.as_str() {
                    "layer" => {
                        current_layer += 1;
                    }
                    "sprite" => {
                        let sprite = parse_sprite(reader, e, &frame_buffer)?;
                        sprites.push(sprite);
                        frame_buffer.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                let tag = tag_name(e)?;
                match tag.as_str() {
                    "frame" => {
                        let frame = parse_frame(&collect_attrs(e)?, base_path)?;
                        frame_buffer.push(frame.clone());
                        frames.push(frame);
                    }
                    "entity" => {
                        let entity = parse_entity(&collect_attrs(e)?, current_layer)?;
                        entities.push(entity);
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                if tag_name_end(e)? == "skin" {
                    break;
                }
            }
            Ok(Event::Eof) => break,
            Err(err) => return Err(XmlError::Parse(err.to_string())),
            _ => {}
        }
        buf.clear();
    }

    Ok(SkinDef {
        name,
        width,
        height,
        judgment_line_y,
        frames,
        sprites,
        entities,
    })
}

fn parse_sprite<R: std::io::BufRead>(
    reader: &mut Reader<R>,
    sprite_event: &quick_xml::events::BytesStart<'_>,
    frame_buffer: &[FrameDef],
) -> Result<SpriteDef, XmlError> {
    let attrs = collect_attrs(sprite_event)?;

    let id = get_attr_required(&attrs, "id")?.to_string();
    let frame_speed_ms = parse_attr_u32_default(&attrs, "framespeed", 50);
    let loop_animation = parse_attr_bool(&attrs, "loop", true);

    let mut frame_refs = Vec::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(ref e)) => {
                if tag_name(e)? == "frame" {
                    let e_attrs = collect_attrs(e)?;
                    if let Some(ref_val) = get_attr(&e_attrs, "ref") {
                        frame_refs.push(ref_val.to_string());
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                if tag_name_end(e)? == "sprite" {
                    break;
                }
            }
            Ok(Event::Eof) => break,
            Err(err) => return Err(XmlError::Parse(err.to_string())),
            _ => {}
        }
        buf.clear();
    }

    // Verify frame refs exist (allow unresolved for flexibility)
    for ref_name in &frame_refs {
        if !frame_buffer.iter().any(|f| f.id == *ref_name) {
            // Allow unresolved refs — they may be defined elsewhere
        }
    }

    Ok(SpriteDef {
        id,
        frame_refs,
        frame_speed_ms,
        loop_animation,
    })
}

fn parse_frame(attrs: &[quick_xml::events::attributes::Attribute<'_>], base_path: &Path) -> Result<FrameDef, XmlError> {
    let id = get_attr_required(attrs, "id")?.to_string();
    let file = PathBuf::from(get_attr_required(attrs, "file")?);
    let x = parse_attr_u32(attrs, "x")?;
    let y = parse_attr_u32(attrs, "y")?;
    let w = parse_attr_u32(attrs, "w")?;
    let h = parse_attr_u32(attrs, "h")?;
    let scale = parse_attr_f32(attrs, "scale", 1.0)?;
    let alpha = parse_attr_f32(attrs, "alpha", 1.0)?;

    // Resolve file path relative to base_path
    let file = if file.is_absolute() {
        file
    } else {
        base_path.join(&file)
    };

    Ok(FrameDef { id, file, x, y, w, h, scale, alpha })
}

fn parse_entity(attrs: &[quick_xml::events::attributes::Attribute<'_>], layer: u32) -> Result<EntityDef, XmlError> {
    let id = get_attr_required(attrs, "id")?.to_string();
    let sprite = get_attr(attrs, "sprite").map(String::from);
    let x = parse_attr_i32(attrs, "x", 0)?;
    let y = parse_attr_i32(attrs, "y", 0)?;
    let head_sprite = get_attr(attrs, "head").map(String::from);
    let body_sprite = get_attr(attrs, "body").map(String::from);
    let tail_sprite = get_attr(attrs, "tail").map(String::from);
    let entity_layer = parse_attr_u32(attrs, "layer").unwrap_or(layer);

    Ok(EntityDef {
        id,
        sprite,
        x,
        y,
        head_sprite,
        body_sprite,
        tail_sprite,
        layer: entity_layer,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SKIN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Resources>
  <skin name="test_skin" width="800" height="600" judgment_line="480">
    <layer>
      <frame id="note1" file="notes.png" x="0" y="0" w="32" h="25" scale="1.0"/>
      <frame id="note1_pressed" file="notes.png" x="32" y="0" w="32" h="25"/>
      <frame id="long_body" file="notes.png" x="64" y="0" w="32" h="8"/>
      <frame id="long_tail" file="notes.png" x="96" y="0" w="32" h="10"/>
      <entity id="NOTE_1" sprite="note1_sprite" x="100"/>
      <entity id="NOTE_2" sprite="note1_sprite" x="200"/>
      <entity id="LONG_NOTE_1" head="note1_sprite" body="long_body" tail="long_tail" x="100"/>
    </layer>
    <layer>
      <sprite id="note1_sprite" framespeed="50" loop="true">
        <frame ref="note1"/>
        <frame ref="note1_pressed"/>
      </sprite>
      <entity id="JUDGMENT_LINE" sprite="judgment_area" x="0" y="480"/>
    </layer>
  </skin>
</Resources>"#;

    #[test]
    fn test_parse_skin_header() {
        let resources = parse_str(TEST_SKIN, Path::new("/test/path")).expect("Failed to parse skin XML");
        let skin = resources.get_skin("test_skin").expect("Skin not found");

        assert_eq!(skin.name, "test_skin");
        assert_eq!(skin.width, 800);
        assert_eq!(skin.height, 600);
        assert_eq!(skin.judgment_line_y, 480);
    }

    #[test]
    fn test_parse_frames() {
        let resources = parse_str(TEST_SKIN, Path::new("/test/path")).expect("Failed to parse skin XML");
        let skin = resources.get_skin("test_skin").expect("Skin not found");

        assert_eq!(skin.frames.len(), 4);
        let note1 = skin.frames.iter().find(|f| f.id == "note1").expect("note1 frame not found");
        assert_eq!(note1.x, 0);
        assert_eq!(note1.y, 0);
        assert_eq!(note1.w, 32);
        assert_eq!(note1.h, 25);
        assert!((note1.scale - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_parse_sprites() {
        let resources = parse_str(TEST_SKIN, Path::new("/test/path")).expect("Failed to parse skin XML");
        let skin = resources.get_skin("test_skin").expect("Skin not found");

        assert_eq!(skin.sprites.len(), 1);
        let sprite = &skin.sprites[0];
        assert_eq!(sprite.id, "note1_sprite");
        assert_eq!(sprite.frame_refs, vec!["note1", "note1_pressed"]);
        assert_eq!(sprite.frame_speed_ms, 50);
        assert!(sprite.loop_animation);
    }

    #[test]
    fn test_parse_entities() {
        let resources = parse_str(TEST_SKIN, Path::new("/test/path")).expect("Failed to parse skin XML");
        let skin = resources.get_skin("test_skin").expect("Skin not found");

        assert_eq!(skin.entities.len(), 4);

        let note1 = skin.entities.iter().find(|e| e.id == "NOTE_1").expect("NOTE_1 not found");
        assert_eq!(note1.sprite.as_deref(), Some("note1_sprite"));
        assert_eq!(note1.x, 100);
        assert_eq!(note1.layer, 1); // layer is 1-indexed (first <layer> tag increments to 1)

        let long_note = skin.entities.iter().find(|e| e.id == "LONG_NOTE_1").expect("LONG_NOTE_1 not found");
        assert_eq!(long_note.head_sprite.as_deref(), Some("note1_sprite"));
        assert_eq!(long_note.body_sprite.as_deref(), Some("long_body"));
        assert_eq!(long_note.tail_sprite.as_deref(), Some("long_tail"));

        let judgment = skin.entities.iter().find(|e| e.id == "JUDGMENT_LINE").expect("JUDGMENT_LINE not found");
        assert_eq!(judgment.y, 480);
        assert_eq!(judgment.layer, 2); // second layer
    }

    #[test]
    fn test_parse_missing_skin_name() {
        let xml = r#"<Resources><skin width="800" height="600" judgment_line="480"></skin></Resources>"#;
        let result = parse_str(xml, Path::new(""));
        assert!(matches!(result, Err(XmlError::MissingAttribute(_))));
    }

    #[test]
    fn test_parse_multiple_skins() {
        let xml = r#"<?xml version="1.0"?>
<Resources>
  <skin name="skin_a" width="800" height="600" judgment_line="480">
    <layer><entity id="NOTE_1" x="100"/></layer>
  </skin>
  <skin name="skin_b" width="1024" height="768" judgment_line="600">
    <layer><entity id="NOTE_1" x="200"/></layer>
  </skin>
</Resources>"#;
        let resources = parse_str(xml, Path::new("")).expect("Failed to parse");
        assert_eq!(resources.skins.len(), 2);
        assert!(resources.get_skin("skin_a").is_some());
        assert!(resources.get_skin("skin_b").is_some());
        assert_eq!(resources.get_skin("skin_b").unwrap().width, 1024);
    }

    #[test]
    fn test_frame_default_alpha() {
        let xml = r#"<?xml version="1.0"?>
<Resources>
  <skin name="test" width="800" height="600" judgment_line="480">
    <layer>
      <frame id="f1" file="test.png" x="0" y="0" w="10" h="10"/>
    </layer>
  </skin>
</Resources>"#;
        let resources = parse_str(xml, Path::new("")).expect("Failed to parse");
        let skin = resources.get_skin("test").unwrap();
        assert_eq!(skin.frames.len(), 1);
        assert!((skin.frames[0].alpha - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_entity_layer_override() {
        let xml = r#"<?xml version="1.0"?>
<Resources>
  <skin name="test" width="800" height="600" judgment_line="480">
    <layer>
      <entity id="NOTE_1" x="100" layer="5"/>
    </layer>
  </skin>
</Resources>"#;
        let resources = parse_str(xml, Path::new("")).expect("Failed to parse");
        let skin = resources.get_skin("test").unwrap();
        assert_eq!(skin.entities[0].layer, 5);
    }

    #[test]
    fn test_skin_names_accessor() {
        let resources = parse_str(TEST_SKIN, Path::new("")).expect("Failed to parse");
        let names = resources.skin_names();
        assert_eq!(names.len(), 1);
        assert_eq!(names[0], "test_skin");
    }

    #[test]
    fn test_file_path_resolution() {
        let xml = r#"<?xml version="1.0"?>
<Resources>
  <skin name="test" width="800" height="600" judgment_line="480">
    <layer>
      <frame id="f1" file="sprites/notes.png" x="0" y="0" w="10" h="10"/>
    </layer>
  </skin>
</Resources>"#;
        let resources = parse_str(xml, Path::new("/my/skin/path")).expect("Failed to parse");
        let skin = resources.get_skin("test").unwrap();
        let frame = &skin.frames[0];
        assert_eq!(frame.file, Path::new("/my/skin/path/sprites/notes.png"));
    }
}
