//! Skin XML parser using quick-xml.
//!
//! Parses the actual open2jam format:
//! - `<spriteset>` with `<sprite id="...">` containing `<frame x y w h file>`
//! - `<skin>` with `<layer>` containing `<entity id="..." sprite="..." x y>`

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use quick_xml::events::Event;
use quick_xml::Reader;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A rectangular frame within a sprite (one animation frame).
#[derive(Debug, Clone)]
pub struct FrameDef {
    pub file: PathBuf,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub scale: f32,
    pub alpha: f32,
}

/// A sprite — a sequence of frames (for animation).
#[derive(Debug, Clone)]
pub struct SpriteDef {
    pub id: String,
    pub frames: Vec<FrameDef>,
    pub frame_speed_ms: u32,
    pub alpha: bool,
}

/// A game entity definition from the skin XML.
#[derive(Debug, Clone)]
pub struct EntityDef {
    pub id: Option<String>,
    pub sprite: Option<String>,
    pub head_sprite: Option<String>, // For long notes
    pub body_sprite: Option<String>, // For long notes
    pub tail_sprite: Option<String>, // For long notes
    pub x: i32,
    pub y: i32,
    pub layer: u32,
}

/// A complete skin definition from the XML.
#[derive(Debug, Clone)]
pub struct SkinDef {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub judgment_line_y: u32,
    pub entities: Vec<EntityDef>,
}

/// All resources parsed from the XML file.
#[derive(Debug, Clone)]
pub struct Resources {
    /// Global sprites (shared across skins).
    pub sprites: HashMap<String, SpriteDef>,
    /// Named skins.
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
}

// ---------------------------------------------------------------------------
// Attribute parsing helpers
// ---------------------------------------------------------------------------

fn get_attr<'a>(
    attrs: &'a [quick_xml::events::attributes::Attribute<'a>],
    name: &str,
) -> Option<&'a str> {
    attrs
        .iter()
        .find(|a| a.key.as_ref() == name.as_bytes())
        .and_then(|a| std::str::from_utf8(&a.value).ok())
}

fn parse_attr_u32(
    attrs: &[quick_xml::events::attributes::Attribute<'_>],
    name: &str,
    default: u32,
) -> u32 {
    get_attr(attrs, name)
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn parse_attr_i32(
    attrs: &[quick_xml::events::attributes::Attribute<'_>],
    name: &str,
    default: i32,
) -> i32 {
    get_attr(attrs, name)
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn parse_attr_f32(
    attrs: &[quick_xml::events::attributes::Attribute<'_>],
    name: &str,
    default: f32,
) -> f32 {
    get_attr(attrs, name)
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn parse_attr_bool(
    attrs: &[quick_xml::events::attributes::Attribute<'_>],
    name: &str,
    default: bool,
) -> bool {
    get_attr(attrs, name)
        .map(|v| matches!(v.to_lowercase().as_str(), "true" | "1" | "yes"))
        .unwrap_or(default)
}

fn collect_attrs<'a>(
    e: &'a quick_xml::events::BytesStart<'a>,
) -> Result<Vec<quick_xml::events::attributes::Attribute<'a>>, XmlError> {
    e.attributes()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| XmlError::Parse(err.to_string()))
}

fn tag_name(e: &quick_xml::events::BytesStart<'_>) -> Result<String, XmlError> {
    std::str::from_utf8(e.name().as_ref())
        .map_err(|err| XmlError::Parse(err.to_string()))
        .map(String::from)
}

fn tag_name_end(e: &quick_xml::events::BytesEnd<'_>) -> Result<String, XmlError> {
    std::str::from_utf8(e.name().as_ref())
        .map_err(|err| XmlError::Parse(err.to_string()))
        .map(String::from)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn parse_file(path: impl AsRef<Path>) -> Result<Resources, XmlError> {
    let path = path.as_ref();
    let data = std::fs::read_to_string(path)?;
    let base_path = path.parent().unwrap_or(Path::new("")).to_path_buf();
    parse_str(&data, &base_path)
}

pub fn parse_str(xml: &str, base_path: &Path) -> Result<Resources, XmlError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut sprites: HashMap<String, SpriteDef> = HashMap::new();
    let mut skins: HashMap<String, SkinDef> = HashMap::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let tag = tag_name(e)?;
                match tag.as_str() {
                    "sprite" => {
                        let sprite = parse_sprite(&mut reader, e, base_path)?;
                        sprites.insert(sprite.id.clone(), sprite);
                    }
                    "skin" => {
                        let skin = parse_skin(&mut reader, e)?;
                        skins.insert(skin.name.clone(), skin);
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(err) => return Err(XmlError::Parse(err.to_string())),
            _ => {}
        }
        buf.clear();
    }

    Ok(Resources {
        sprites,
        skins,
        base_path: base_path.to_path_buf(),
    })
}

// ---------------------------------------------------------------------------
// Sprite parsing
// ---------------------------------------------------------------------------

fn parse_sprite<R: std::io::BufRead>(
    reader: &mut Reader<R>,
    sprite_event: &quick_xml::events::BytesStart<'_>,
    base_path: &Path,
) -> Result<SpriteDef, XmlError> {
    let attrs = collect_attrs(sprite_event)?;
    let id = get_attr(&attrs, "id")
        .unwrap_or("unnamed_sprite")
        .to_string();
    // framespeed in XML is in FPS (e.g., 60 = 60fps), convert to ms per frame
    // Java code: framespeed /= 1000, then sub_frame += delta_ms * (framespeed/1000)
    // So ms_per_frame = 1000 / framespeed_value
    let framespeed_value = parse_attr_f32(&attrs, "framespeed", 50.0);
    let frame_speed_ms = if framespeed_value > 0.0 {
        (1000.0 / framespeed_value) as u32
    } else {
        50
    };
    let alpha = parse_attr_bool(&attrs, "alpha", false);

    let mut frames: Vec<FrameDef> = Vec::new();
    let mut buf = Vec::new();

    // Read child <frame> elements
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(ref e)) => {
                let tag = tag_name(e)?;
                if tag == "frame" {
                    let f_attrs = collect_attrs(e)?;
                    let file = PathBuf::from(get_attr(&f_attrs, "file").unwrap_or(""));
                    let x = parse_attr_u32(&f_attrs, "x", 0);
                    let y = parse_attr_u32(&f_attrs, "y", 0);
                    let w = parse_attr_u32(&f_attrs, "w", 0);
                    let h = parse_attr_u32(&f_attrs, "h", 0);
                    let scale = parse_attr_f32(&f_attrs, "scale", 1.0);
                    let alpha_val = parse_attr_f32(&f_attrs, "alpha", 1.0);
                    let file = if file.is_absolute() {
                        file
                    } else {
                        base_path.join(&file)
                    };
                    frames.push(FrameDef {
                        file,
                        x,
                        y,
                        w,
                        h,
                        scale,
                        alpha: alpha_val,
                    });
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

    Ok(SpriteDef {
        id,
        frames,
        frame_speed_ms,
        alpha,
    })
}

// ---------------------------------------------------------------------------
// Skin parsing
// ---------------------------------------------------------------------------

fn parse_skin<R: std::io::BufRead>(
    reader: &mut Reader<R>,
    skin_event: &quick_xml::events::BytesStart<'_>,
) -> Result<SkinDef, XmlError> {
    let attrs = collect_attrs(skin_event)?;
    let name = get_attr(&attrs, "name").unwrap_or("unnamed").to_string();
    let width = parse_attr_u32(&attrs, "width", 800);
    let height = parse_attr_u32(&attrs, "height", 600);
    let judgment_line_y = parse_attr_u32(&attrs, "judgment_line", 480);

    let mut entities: Vec<EntityDef> = Vec::new();
    let mut current_layer: u32 = 0;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag = tag_name(e)?;
                if tag == "layer" {
                    current_layer += 1;
                }
            }
            Ok(Event::Empty(ref e)) => {
                let tag = tag_name(e)?;
                if tag == "entity" {
                    let e_attrs = collect_attrs(e)?;
                    let id = get_attr(&e_attrs, "id").map(String::from);
                    let sprite = get_attr(&e_attrs, "sprite").map(String::from);
                    let head_sprite = get_attr(&e_attrs, "head").map(String::from);
                    let body_sprite = get_attr(&e_attrs, "body").map(String::from);
                    let tail_sprite = get_attr(&e_attrs, "tail").map(String::from);
                    let x = parse_attr_i32(&e_attrs, "x", 0);
                    let y = parse_attr_i32(&e_attrs, "y", 0);
                    entities.push(EntityDef {
                        id,
                        sprite,
                        head_sprite,
                        body_sprite,
                        tail_sprite,
                        x,
                        y,
                        layer: current_layer,
                    });
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
        entities,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_real_skin() {
        let resources =
            parse_file("resources/resources.xml")
                .expect("Failed to parse skin XML");

        // Check sprites are loaded
        assert!(
            resources.sprites.contains_key("note_bg"),
            "note_bg sprite not found"
        );
        assert!(
            resources.sprites.contains_key("head_note_white"),
            "head_note_white not found"
        );
        assert!(
            resources.sprites.contains_key("judgmentarea"),
            "judgmentarea not found"
        );
        assert!(
            resources.sprites.contains_key("measure_mark"),
            "measure_mark not found"
        );

        // Check skin is loaded
        let skin = resources.get_skin("o2jam").expect("Skin o2jam not found");
        assert_eq!(skin.width, 800);
        assert_eq!(skin.height, 600);
        assert_eq!(skin.judgment_line_y, 480);
        assert!(!skin.entities.is_empty());

        // Check entities
        let note_1 = skin
            .entities
            .iter()
            .find(|e| e.id.as_deref() == Some("NOTE_1"));
        assert!(note_1.is_some(), "NOTE_1 entity not found");
    }

    #[test]
    fn test_parse_sprites() {
        let resources =
            parse_file("resources/resources.xml")
                .expect("Failed to parse skin XML");

        let head_white = resources
            .sprites
            .get("head_note_white")
            .expect("head_note_white not found");
        assert_eq!(head_white.frames.len(), 3); // 3 animation frames
        assert_eq!(head_white.frame_speed_ms, 12);

        let judgment = resources
            .sprites
            .get("judgmentarea")
            .expect("judgmentarea not found");
        assert_eq!(judgment.frames.len(), 2);
    }

    #[test]
    fn test_parse_skin_header() {
        let xml = r#"<Resources><skin name="test" width="800" height="600" judgment_line="480">
<layer><entity id="NOTE_1" sprite="note1" x="100"/></layer>
</skin></Resources>"#;
        let resources = parse_str(xml, Path::new("")).expect("Failed to parse");
        let skin = resources.get_skin("test").expect("Skin not found");
        assert_eq!(skin.width, 800);
        assert_eq!(skin.judgment_line_y, 480);
        assert_eq!(skin.entities.len(), 1);
        assert_eq!(skin.entities[0].id.as_deref(), Some("NOTE_1"));
        assert_eq!(skin.entities[0].sprite.as_deref(), Some("note1"));
    }
}
