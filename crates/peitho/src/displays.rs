use serde::Deserialize;
use std::process::Command;

pub const MACOS_DISPLAY_JXA: &str = r#"
ObjC.import('AppKit');
const screens = $.NSScreen.screens;
const out = [];
for (let i = 0; i < screens.count; i++) {
  const frame = screens.objectAtIndex(i).frame;
  out.push({
    x: Math.round(frame.origin.x),
    y: Math.round(frame.origin.y),
    width: Math.round(frame.size.width),
    height: Math.round(frame.size.height)
  });
}
JSON.stringify(out);
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChromeDisplay {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowPlacement {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub fullscreen: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentationLayout {
    pub slides: WindowPlacement,
    pub presenter: WindowPlacement,
}

#[derive(Debug, Deserialize)]
struct NsscreenFrame {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

pub fn parse_nsscreen_json(json: &str) -> Result<Vec<ChromeDisplay>, serde_json::Error> {
    let frames: Vec<NsscreenFrame> = serde_json::from_str(json)?;
    Ok(convert_nsscreen_frames(&frames))
}

fn convert_nsscreen_frames(frames: &[NsscreenFrame]) -> Vec<ChromeDisplay> {
    let primary = frames
        .iter()
        .find(|frame| frame.x == 0 && frame.y == 0)
        .or_else(|| frames.first());
    let Some(primary) = primary else {
        return Vec::new();
    };
    let primary_height = primary.height as i32;

    frames
        .iter()
        .map(|frame| ChromeDisplay {
            x: frame.x,
            y: primary_height - (frame.y + frame.height as i32),
            width: frame.width,
            height: frame.height,
            primary: frame.x == primary.x && frame.y == primary.y,
        })
        .collect()
}

pub fn plan_presentation_layout(displays: &[ChromeDisplay]) -> Option<PresentationLayout> {
    let primary = displays.iter().find(|display| display.primary)?;
    let slides = displays.iter().find(|display| !display.primary)?;

    let presenter_width = 1200_u32.min(primary.width);
    let presenter_height = 800_u32.min(primary.height);
    let presenter_x = primary.x + ((primary.width - presenter_width) / 2) as i32;
    let presenter_y = primary.y + ((primary.height - presenter_height) / 2) as i32;

    Some(PresentationLayout {
        slides: WindowPlacement {
            x: slides.x,
            y: slides.y,
            width: slides.width,
            height: slides.height,
            fullscreen: true,
        },
        presenter: WindowPlacement {
            x: presenter_x,
            y: presenter_y,
            width: presenter_width,
            height: presenter_height,
            fullscreen: false,
        },
    })
}

pub fn layout_from_jxa_output(stdout: &str) -> Option<PresentationLayout> {
    let displays = parse_nsscreen_json(stdout).ok()?;
    plan_presentation_layout(&displays)
}

pub fn detect_presentation_layout() -> Option<PresentationLayout> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let output = Command::new("osascript")
        .args(["-l", "JavaScript", "-e", MACOS_DISPLAY_JXA])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    layout_from_jxa_output(&stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_nsscreen_frames_to_chrome_coordinates() {
        let json = r#"[
          {"x":0,"y":0,"width":1512,"height":982},
          {"x":-1055,"y":316,"width":1055,"height":666}
        ]"#;

        let displays = parse_nsscreen_json(json).unwrap();

        assert_eq!(
            displays,
            vec![
                ChromeDisplay {
                    x: 0,
                    y: 0,
                    width: 1512,
                    height: 982,
                    primary: true,
                },
                ChromeDisplay {
                    x: -1055,
                    y: 0,
                    width: 1055,
                    height: 666,
                    primary: false,
                },
            ]
        );
    }

    #[test]
    fn plans_slides_on_external_and_presenter_on_primary() {
        let displays = vec![
            ChromeDisplay {
                x: 0,
                y: 0,
                width: 1512,
                height: 982,
                primary: true,
            },
            ChromeDisplay {
                x: -1055,
                y: 0,
                width: 1055,
                height: 666,
                primary: false,
            },
        ];

        let layout = plan_presentation_layout(&displays).unwrap();

        assert_eq!(
            layout.slides,
            WindowPlacement {
                x: -1055,
                y: 0,
                width: 1055,
                height: 666,
                fullscreen: true,
            }
        );
        assert_eq!(
            layout.presenter,
            WindowPlacement {
                x: 156,
                y: 91,
                width: 1200,
                height: 800,
                fullscreen: false,
            }
        );
    }

    #[test]
    fn clamps_presenter_to_small_primary_display() {
        let displays = vec![
            ChromeDisplay {
                x: 0,
                y: 0,
                width: 900,
                height: 700,
                primary: true,
            },
            ChromeDisplay {
                x: 900,
                y: 0,
                width: 1280,
                height: 720,
                primary: false,
            },
        ];

        let layout = plan_presentation_layout(&displays).unwrap();

        assert_eq!(
            layout.presenter,
            WindowPlacement {
                x: 0,
                y: 0,
                width: 900,
                height: 700,
                fullscreen: false,
            }
        );
    }

    #[test]
    fn returns_none_for_single_display() {
        let displays = vec![ChromeDisplay {
            x: 0,
            y: 0,
            width: 1512,
            height: 982,
            primary: true,
        }];

        assert_eq!(plan_presentation_layout(&displays), None);
    }

    #[test]
    fn jxa_script_mentions_appkit_nsscreen_and_json() {
        assert!(MACOS_DISPLAY_JXA.contains("ObjC.import('AppKit')"));
        assert!(MACOS_DISPLAY_JXA.contains("$.NSScreen.screens"));
        assert!(MACOS_DISPLAY_JXA.contains("JSON.stringify"));
    }

    #[test]
    fn layout_from_jxa_output_returns_none_for_invalid_json() {
        assert_eq!(layout_from_jxa_output("not json"), None);
    }

    #[test]
    fn layout_from_jxa_output_plans_valid_two_display_json() {
        let json = r#"[{"x":0,"y":0,"width":1512,"height":982},{"x":-1055,"y":316,"width":1055,"height":666}]"#;

        assert_eq!(
            layout_from_jxa_output(json).unwrap().slides,
            WindowPlacement {
                x: -1055,
                y: 0,
                width: 1055,
                height: 666,
                fullscreen: true,
            }
        );
    }
}
