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

/// How a browser window should be opened. `Fullscreen` positions the window
/// on a display and fullscreens it there; `Windowed` positions and sizes a
/// normal window explicitly; `Restored` passes no placement flags so Chrome
/// restores the bounds saved in the profile from the last session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowPlacement {
    Fullscreen {
        x: i32,
        y: i32,
    },
    Windowed {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    },
    Restored,
}

/// Presenter window bounds recorded by Chrome in the profile
/// (`browser.app_window_placement`), in Chrome's top-left screen coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SavedWindowBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// How the presenter window should be planned. In `Windowed` mode Chrome is
/// left to restore the saved bounds, but only when they would be visible:
/// without placement flags Chrome may drop a first-run window onto the
/// slides display, where the fullscreen slides Space hides it entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresenterMode {
    Fullscreen,
    Windowed { saved: Option<SavedWindowBounds> },
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

fn contains_point(display: &ChromeDisplay, x: i32, y: i32) -> bool {
    x >= display.x
        && x < display.x + display.width as i32
        && y >= display.y
        && y < display.y + display.height as i32
}

/// Saved bounds are only worth restoring when their center sits on a display
/// that the fullscreen slides Space does not cover; anywhere else the
/// restored window would be hidden or off screen entirely. With no slides
/// display (single-display windowed mode) every display is fine.
fn saved_bounds_visible(
    saved: &SavedWindowBounds,
    displays: &[ChromeDisplay],
    slides: Option<&ChromeDisplay>,
) -> bool {
    let center_x = saved.x + (saved.width / 2) as i32;
    let center_y = saved.y + (saved.height / 2) as i32;
    displays
        .iter()
        .any(|display| Some(display) != slides && contains_point(display, center_x, center_y))
}

pub fn plan_presentation_layout(
    displays: &[ChromeDisplay],
    presenter_mode: PresenterMode,
) -> Option<PresentationLayout> {
    let primary = displays.iter().find(|display| display.primary)?;
    let slides_display = displays.iter().find(|display| !display.primary);

    let presenter_width = 1200_u32.min(primary.width);
    let presenter_height = 800_u32.min(primary.height);
    let presenter_x = primary.x + ((primary.width - presenter_width) / 2) as i32;
    let presenter_y = primary.y + ((primary.height - presenter_height) / 2) as i32;
    let presenter_seed = WindowPlacement::Windowed {
        x: presenter_x,
        y: presenter_y,
        width: presenter_width,
        height: presenter_height,
    };

    match (slides_display, presenter_mode) {
        // Two displays: slides go fullscreen on the external one.
        (Some(slides), mode) => {
            let presenter = match mode {
                PresenterMode::Fullscreen => WindowPlacement::Fullscreen {
                    x: presenter_x,
                    y: presenter_y,
                },
                PresenterMode::Windowed { saved: Some(saved) }
                    if saved_bounds_visible(&saved, displays, Some(slides)) =>
                {
                    WindowPlacement::Restored
                }
                PresenterMode::Windowed { .. } => presenter_seed,
            };
            Some(PresentationLayout {
                slides: WindowPlacement::Fullscreen {
                    x: slides.x,
                    y: slides.y,
                },
                presenter,
            })
        }
        // Single display, presentation mode: no presenter (as before).
        (None, PresenterMode::Fullscreen) => None,
        // Single display, debug mode: open both as normal windows so the
        // presenter can be exercised without a second (virtual) display.
        // The slides window is seeded top-left; nothing goes fullscreen,
        // so a saved presenter placement on this display is restorable.
        (None, PresenterMode::Windowed { saved }) => {
            let presenter = match saved {
                Some(saved) if saved_bounds_visible(&saved, displays, None) => {
                    WindowPlacement::Restored
                }
                _ => presenter_seed,
            };
            Some(PresentationLayout {
                slides: WindowPlacement::Windowed {
                    x: primary.x + 24,
                    y: primary.y + 48,
                    width: 960_u32.min(primary.width),
                    height: 600_u32.min(primary.height),
                },
                presenter,
            })
        }
    }
}

pub fn layout_from_jxa_output(
    stdout: &str,
    presenter_mode: PresenterMode,
) -> Option<PresentationLayout> {
    let displays = parse_nsscreen_json(stdout).ok()?;
    plan_presentation_layout(&displays, presenter_mode)
}

pub fn detect_nsscreen_json() -> Option<String> {
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
    String::from_utf8(output.stdout).ok()
}

pub fn detect_presentation_layout(presenter_mode: PresenterMode) -> Option<PresentationLayout> {
    let stdout = detect_nsscreen_json()?;
    layout_from_jxa_output(&stdout, presenter_mode)
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

    fn two_displays() -> Vec<ChromeDisplay> {
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
    }

    #[test]
    fn plans_slides_on_external_and_presenter_fullscreen_on_primary() {
        let layout = plan_presentation_layout(&two_displays(), PresenterMode::Fullscreen).unwrap();

        assert_eq!(
            layout.slides,
            WindowPlacement::Fullscreen { x: -1055, y: 0 }
        );
        assert_eq!(
            layout.presenter,
            WindowPlacement::Fullscreen { x: 156, y: 91 }
        );
    }

    #[test]
    fn windowed_first_run_seeds_centered_window_on_primary() {
        let layout =
            plan_presentation_layout(&two_displays(), PresenterMode::Windowed { saved: None })
                .unwrap();

        assert_eq!(
            layout.presenter,
            WindowPlacement::Windowed {
                x: 156,
                y: 91,
                width: 1200,
                height: 800,
            }
        );
        assert_eq!(
            layout.slides,
            WindowPlacement::Fullscreen { x: -1055, y: 0 }
        );
    }

    #[test]
    fn windowed_mode_restores_saved_bounds_on_primary() {
        let layout = plan_presentation_layout(
            &two_displays(),
            PresenterMode::Windowed {
                saved: Some(SavedWindowBounds {
                    x: 300,
                    y: 60,
                    width: 1200,
                    height: 900,
                }),
            },
        )
        .unwrap();

        assert_eq!(layout.presenter, WindowPlacement::Restored);
    }

    #[test]
    fn windowed_mode_reseeds_when_saved_bounds_sit_on_slides_display() {
        let layout = plan_presentation_layout(
            &two_displays(),
            PresenterMode::Windowed {
                saved: Some(SavedWindowBounds {
                    x: -1000,
                    y: 47,
                    width: 900,
                    height: 600,
                }),
            },
        )
        .unwrap();

        assert_eq!(
            layout.presenter,
            WindowPlacement::Windowed {
                x: 156,
                y: 91,
                width: 1200,
                height: 800,
            }
        );
    }

    #[test]
    fn windowed_mode_reseeds_when_saved_bounds_are_off_screen() {
        let layout = plan_presentation_layout(
            &two_displays(),
            PresenterMode::Windowed {
                saved: Some(SavedWindowBounds {
                    x: 9000,
                    y: 9000,
                    width: 1200,
                    height: 800,
                }),
            },
        )
        .unwrap();

        assert_eq!(
            layout.presenter,
            WindowPlacement::Windowed {
                x: 156,
                y: 91,
                width: 1200,
                height: 800,
            }
        );
    }

    #[test]
    fn centers_fullscreen_presenter_position_within_small_primary_display() {
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

        let layout = plan_presentation_layout(&displays, PresenterMode::Fullscreen).unwrap();

        assert_eq!(layout.presenter, WindowPlacement::Fullscreen { x: 0, y: 0 });
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

        assert_eq!(
            plan_presentation_layout(&displays, PresenterMode::Fullscreen),
            None
        );
    }

    fn single_display() -> Vec<ChromeDisplay> {
        vec![ChromeDisplay {
            x: 0,
            y: 0,
            width: 1512,
            height: 982,
            primary: true,
        }]
    }

    #[test]
    fn single_display_windowed_opens_both_as_windows() {
        let layout =
            plan_presentation_layout(&single_display(), PresenterMode::Windowed { saved: None })
                .unwrap();

        assert_eq!(
            layout.slides,
            WindowPlacement::Windowed {
                x: 24,
                y: 48,
                width: 960,
                height: 600,
            }
        );
        assert_eq!(
            layout.presenter,
            WindowPlacement::Windowed {
                x: 156,
                y: 91,
                width: 1200,
                height: 800,
            }
        );
    }

    #[test]
    fn single_display_windowed_restores_saved_presenter_bounds() {
        let layout = plan_presentation_layout(
            &single_display(),
            PresenterMode::Windowed {
                saved: Some(SavedWindowBounds {
                    x: 300,
                    y: 60,
                    width: 1180,
                    height: 800,
                }),
            },
        )
        .unwrap();

        assert_eq!(layout.presenter, WindowPlacement::Restored);
        assert!(matches!(layout.slides, WindowPlacement::Windowed { .. }));
    }

    #[test]
    fn single_display_windowed_reseeds_offscreen_saved_bounds() {
        let layout = plan_presentation_layout(
            &single_display(),
            PresenterMode::Windowed {
                saved: Some(SavedWindowBounds {
                    x: 9000,
                    y: 9000,
                    width: 1200,
                    height: 800,
                }),
            },
        )
        .unwrap();

        assert_eq!(
            layout.presenter,
            WindowPlacement::Windowed {
                x: 156,
                y: 91,
                width: 1200,
                height: 800,
            }
        );
    }

    #[test]
    fn jxa_script_mentions_appkit_nsscreen_and_json() {
        assert!(MACOS_DISPLAY_JXA.contains("ObjC.import('AppKit')"));
        assert!(MACOS_DISPLAY_JXA.contains("$.NSScreen.screens"));
        assert!(MACOS_DISPLAY_JXA.contains("JSON.stringify"));
    }

    #[test]
    fn layout_from_jxa_output_returns_none_for_invalid_json() {
        assert_eq!(
            layout_from_jxa_output("not json", PresenterMode::Fullscreen),
            None
        );
    }

    #[test]
    fn layout_from_jxa_output_plans_valid_two_display_json() {
        let json = r#"[{"x":0,"y":0,"width":1512,"height":982},{"x":-1055,"y":316,"width":1055,"height":666}]"#;

        assert_eq!(
            layout_from_jxa_output(json, PresenterMode::Fullscreen)
                .unwrap()
                .slides,
            WindowPlacement::Fullscreen { x: -1055, y: 0 }
        );
    }

    #[test]
    fn layout_from_jxa_output_passes_presenter_mode_through() {
        let json = r#"[{"x":0,"y":0,"width":1512,"height":982},{"x":-1055,"y":316,"width":1055,"height":666}]"#;
        let saved = SavedWindowBounds {
            x: 200,
            y: 100,
            width: 1200,
            height: 800,
        };

        assert_eq!(
            layout_from_jxa_output(json, PresenterMode::Windowed { saved: Some(saved) })
                .unwrap()
                .presenter,
            WindowPlacement::Restored
        );
    }
}
