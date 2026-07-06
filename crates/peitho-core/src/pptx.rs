use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Cursor, Write},
    path::Path,
};

use html_escape::{encode_double_quoted_attribute, encode_text};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

use crate::{
    domain::{
        MeasuredDeck, MeasuredImage, MeasuredParagraph, MeasuredRect, MeasuredRun, MeasuredSlide,
        RenderedSlide, ResolvedImageAsset,
    },
    error::{BuildError, ErrorKind, Result},
    phase::{Deck, Rendered},
};

const EMU_PER_PX: f64 = 9525.0;

pub fn build_pptx(
    measured: &MeasuredDeck,
    deck: &Deck<Rendered>,
    image_assets: &[ResolvedImageAsset],
) -> Result<Vec<u8>> {
    let prepared = prepare_pptx(measured, deck, image_assets)?;
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut cursor);
        write_zip_file(
            &mut zip,
            "[Content_Types].xml",
            content_types_xml(&prepared),
        )?;
        write_zip_file(&mut zip, "_rels/.rels", root_rels_xml())?;
        write_zip_file(&mut zip, "docProps/core.xml", core_props_xml())?;
        write_zip_file(&mut zip, "docProps/app.xml", app_props_xml(&prepared))?;
        write_zip_file(
            &mut zip,
            "ppt/presentation.xml",
            presentation_xml(&prepared),
        )?;
        write_zip_file(
            &mut zip,
            "ppt/_rels/presentation.xml.rels",
            presentation_rels_xml(&prepared),
        )?;
        write_zip_file(
            &mut zip,
            "ppt/slideMasters/slideMaster1.xml",
            slide_master_xml(),
        )?;
        write_zip_file(
            &mut zip,
            "ppt/slideMasters/_rels/slideMaster1.xml.rels",
            slide_master_rels_xml(),
        )?;
        write_zip_file(
            &mut zip,
            "ppt/slideLayouts/slideLayout1.xml",
            slide_layout_xml(),
        )?;
        write_zip_file(
            &mut zip,
            "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
            slide_layout_rels_xml(),
        )?;
        write_zip_file(&mut zip, "ppt/theme/theme1.xml", theme_xml())?;
        if prepared.has_notes {
            write_zip_file(
                &mut zip,
                "ppt/notesMasters/notesMaster1.xml",
                notes_master_xml(),
            )?;
            write_zip_file(
                &mut zip,
                "ppt/notesMasters/_rels/notesMaster1.xml.rels",
                notes_master_rels_xml(),
            )?;
        }
        for slide in &prepared.slides {
            write_zip_file(
                &mut zip,
                &format!("ppt/slides/slide{}.xml", slide.number),
                slide_xml(slide)?,
            )?;
            write_zip_file(
                &mut zip,
                &format!("ppt/slides/_rels/slide{}.xml.rels", slide.number),
                slide_rels_xml(slide),
            )?;
            if let Some(notes) = &slide.notes {
                write_zip_file(
                    &mut zip,
                    &format!("ppt/notesSlides/notesSlide{}.xml", slide.number),
                    notes_slide_xml(slide.number, notes)?,
                )?;
                write_zip_file(
                    &mut zip,
                    &format!("ppt/notesSlides/_rels/notesSlide{}.xml.rels", slide.number),
                    notes_slide_rels_xml(slide.number),
                )?;
            }
        }
        for media in &prepared.media {
            write_zip_bytes(
                &mut zip,
                &format!("ppt/media/{}", media.media_name),
                &media.bytes,
            )?;
        }
        zip.finish().map_err(pptx_io_error)?;
    }
    Ok(cursor.into_inner())
}

#[derive(Debug)]
struct PreparedPptx {
    canvas_width: f64,
    canvas_height: f64,
    slides: Vec<PreparedSlide>,
    media: Vec<PreparedMedia>,
    media_extensions: BTreeSet<String>,
    has_notes: bool,
}

#[derive(Debug)]
struct PreparedSlide {
    number: usize,
    measured: MeasuredSlide,
    notes: Option<String>,
    images: Vec<PreparedImage>,
}

#[derive(Debug)]
struct PreparedImage {
    measured: MeasuredImage,
    media_name: String,
}

#[derive(Debug)]
struct PreparedMedia {
    media_name: String,
    bytes: Vec<u8>,
}

fn prepare_pptx(
    measured: &MeasuredDeck,
    deck: &Deck<Rendered>,
    image_assets: &[ResolvedImageAsset],
) -> Result<PreparedPptx> {
    if measured.slides.len() != deck.slide_count() {
        return Err(pptx_error(
            format!(
                "measured slide count {} does not match rendered slide count {}",
                measured.slides.len(),
                deck.slide_count()
            ),
            "rerun measurement and keep the rendered deck in sync",
        ));
    }

    let assets_by_src = image_assets
        .iter()
        .map(|asset| (asset.dist_rel.as_str().to_owned(), asset))
        .collect::<BTreeMap<_, _>>();
    let mut media_index = 1;
    let mut media_extensions = BTreeSet::new();
    let mut media_by_source: BTreeMap<std::path::PathBuf, String> = BTreeMap::new();
    let mut media = Vec::new();
    let mut slides = Vec::with_capacity(measured.slides.len());
    for (index, (measured_slide, rendered_slide)) in
        measured.slides.iter().zip(deck.slides()).enumerate()
    {
        validate_slide_key(index, measured_slide, rendered_slide)?;
        let mut images = Vec::new();
        for image in &measured_slide.images {
            let asset = assets_by_src.get(&image.src).ok_or_else(|| {
                pptx_error(
                    format!("measured image {} has no resolved image asset", image.src),
                    "ensure measurement uses rendered dist-relative image paths",
                )
            })?;
            let extension = media_extension(&image.src, &asset.source_abs)?;
            image_content_type(&extension)?;
            media_extensions.insert(extension.clone());
            let media_name = if let Some(media_name) = media_by_source.get(&asset.source_abs) {
                media_name.clone()
            } else {
                let media_name = format!("image{media_index}.{extension}");
                let bytes = fs::read(&asset.source_abs).map_err(|err| {
                    pptx_error(
                        format!(
                            "failed to read image asset {}: {err}",
                            asset.source_abs.display()
                        ),
                        "make sure resolved image assets remain readable during pptx export",
                    )
                })?;
                media.push(PreparedMedia {
                    media_name: media_name.clone(),
                    bytes,
                });
                media_by_source.insert(asset.source_abs.clone(), media_name.clone());
                media_index += 1;
                media_name
            };
            images.push(PreparedImage {
                measured: image.clone(),
                media_name,
            });
        }
        slides.push(PreparedSlide {
            number: index + 1,
            measured: measured_slide.clone(),
            notes: rendered_slide
                .notes()
                .map(str::to_owned)
                .filter(|s| !s.is_empty()),
            images,
        });
    }
    let has_notes = slides.iter().any(|slide| slide.notes.is_some());
    Ok(PreparedPptx {
        canvas_width: measured.canvas_width,
        canvas_height: measured.canvas_height,
        slides,
        media,
        media_extensions,
        has_notes,
    })
}

fn validate_slide_key(
    index: usize,
    measured_slide: &MeasuredSlide,
    rendered_slide: &RenderedSlide,
) -> Result<()> {
    if measured_slide.key == rendered_slide.key().as_str() {
        return Ok(());
    }
    Err(pptx_error(
        format!(
            "measured slide key '{}' does not match rendered slide key '{}' at index {}",
            measured_slide.key,
            rendered_slide.key().as_str(),
            index
        ),
        "rerun measurement against the same rendered deck",
    ))
}

fn write_zip_file(
    zip: &mut ZipWriter<&mut Cursor<Vec<u8>>>,
    path: &str,
    content: String,
) -> Result<()> {
    write_zip_entry(zip, path, content.as_bytes(), CompressionMethod::Deflated)
}

fn write_zip_bytes(
    zip: &mut ZipWriter<&mut Cursor<Vec<u8>>>,
    path: &str,
    bytes: &[u8],
) -> Result<()> {
    write_zip_entry(zip, path, bytes, CompressionMethod::Stored)
}

fn write_zip_entry(
    zip: &mut ZipWriter<&mut Cursor<Vec<u8>>>,
    path: &str,
    bytes: &[u8],
    compression: CompressionMethod,
) -> Result<()> {
    let options = SimpleFileOptions::default().compression_method(compression);
    zip.start_file(path, options).map_err(pptx_io_error)?;
    zip.write_all(bytes).map_err(pptx_io_error)
}

fn content_types_xml(pptx: &PreparedPptx) -> String {
    let mut defaults = String::new();
    for extension in &pptx.media_extensions {
        let content_type = image_content_type(extension).unwrap_or("application/octet-stream");
        defaults.push_str(&format!(
            r#"<Default Extension="{}" ContentType="{}"/>"#,
            xml_attr(extension),
            content_type
        ));
    }
    let mut overrides = String::new();
    for slide in &pptx.slides {
        overrides.push_str(&format!(
            r#"<Override PartName="/ppt/slides/slide{}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>"#,
            slide.number
        ));
        if slide.notes.is_some() {
            overrides.push_str(&format!(
                r#"<Override PartName="/ppt/notesSlides/notesSlide{}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml"/>"#,
                slide.number
            ));
        }
    }
    if pptx.has_notes {
        overrides.push_str(r#"<Override PartName="/ppt/notesMasters/notesMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.notesMaster+xml"/>"#);
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>{defaults}<Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/><Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/><Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/><Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/>{overrides}</Types>"#
    )
}

fn root_rels_xml() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties" Target="docProps/app.xml"/></Relationships>"#.to_owned()
}

fn core_props_xml() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:dcmitype="http://purl.org/dc/dcmitype/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><dc:creator>peitho</dc:creator><cp:lastModifiedBy>peitho</cp:lastModifiedBy><dcterms:created xsi:type="dcterms:W3CDTF">2026-07-06T00:00:00Z</dcterms:created><dcterms:modified xsi:type="dcterms:W3CDTF">2026-07-06T00:00:00Z</dcterms:modified></cp:coreProperties>"#.to_owned()
}

fn app_props_xml(pptx: &PreparedPptx) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties" xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes"><Application>peitho</Application><PresentationFormat>On-screen Show</PresentationFormat><Slides>{}</Slides><Notes>{}</Notes></Properties>"#,
        pptx.slides.len(),
        pptx.slides
            .iter()
            .filter(|slide| slide.notes.is_some())
            .count()
    )
}

fn presentation_xml(pptx: &PreparedPptx) -> String {
    let slide_ids = pptx
        .slides
        .iter()
        .enumerate()
        .map(|(index, _)| format!(r#"<p:sldId id="{}" r:id="rId{}"/>"#, 256 + index, index + 2))
        .collect::<String>();
    let cx = px_to_emu(pptx.canvas_width);
    let cy = px_to_emu(pptx.canvas_height);
    let notes_master_ids = if pptx.has_notes {
        format!(
            r#"<p:notesMasterIdLst><p:notesMasterId r:id="rId{}"/></p:notesMasterIdLst>"#,
            pptx.slides.len() + 2
        )
    } else {
        String::new()
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:presentation xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst>{notes_master_ids}<p:sldIdLst>{slide_ids}</p:sldIdLst><p:sldSz cx="{cx}" cy="{cy}"/><p:notesSz cx="6858000" cy="9144000"/><p:defaultTextStyle/></p:presentation>"#
    )
}

fn presentation_rels_xml(pptx: &PreparedPptx) -> String {
    let mut rels = String::from(
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="slideMasters/slideMaster1.xml"/>"#,
    );
    for slide in &pptx.slides {
        rels.push_str(&format!(
            r#"<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide{}.xml"/>"#,
            slide.number + 1,
            slide.number
        ));
    }
    if pptx.has_notes {
        rels.push_str(&format!(
            r#"<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesMaster" Target="notesMasters/notesMaster1.xml"/>"#,
            pptx.slides.len() + 2
        ));
    }
    relationships_xml(&rels)
}

fn slide_xml(slide: &PreparedSlide) -> Result<String> {
    let background = slide_background_xml(&slide.measured.background_color)?;
    let mut shapes = String::new();
    let mut shape_id = 2;
    for measured_box in &slide.measured.boxes {
        shapes.push_str(&text_shape_xml(shape_id, measured_box)?);
        shape_id += 1;
    }
    for (index, image) in slide.images.iter().enumerate() {
        shapes.push_str(&picture_xml(
            shape_id,
            &image.measured,
            &format!("rId{}", index + 2),
        ));
        shape_id += 1;
    }
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld>{background}<p:spTree>{group_shape_xml}{shapes}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"#,
        group_shape_xml = group_shape_xml(),
    ))
}

fn slide_rels_xml(slide: &PreparedSlide) -> String {
    let mut rels = String::from(
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/>"#,
    );
    for (index, image) in slide.images.iter().enumerate() {
        rels.push_str(&format!(
            r#"<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/{}"/>"#,
            index + 2,
            xml_attr(&image.media_name)
        ));
    }
    if slide.notes.is_some() {
        rels.push_str(&format!(
            r#"<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide" Target="../notesSlides/notesSlide{}.xml"/>"#,
            slide.images.len() + 2,
            slide.number
        ));
    }
    relationships_xml(&rels)
}

fn text_shape_xml(shape_id: usize, measured_box: &crate::domain::MeasuredBox) -> Result<String> {
    let rect = xfrm_xml(&measured_box.rect);
    let fill = fill_xml(&measured_box.style.background_color)?;
    let line = line_xml(
        &measured_box.style.border_color,
        measured_box.style.border_width,
    )?;
    let geometry = if measured_box.style.border_radius > 0.0 {
        r#"<a:prstGeom prst="roundRect"><a:avLst/></a:prstGeom>"#
    } else {
        r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom>"#
    };
    let paragraphs = measured_box
        .paragraphs
        .iter()
        .map(paragraph_xml)
        .collect::<Result<String>>()?;
    Ok(format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="{shape_id}" name="{name}"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr>{rect}{geometry}{fill}{line}</p:spPr><p:txBody><a:bodyPr wrap="square" anchor="t" lIns="0" tIns="0" rIns="0" bIns="0"><a:noAutofit/></a:bodyPr><a:lstStyle/>{paragraphs}</p:txBody></p:sp>"#,
        name = xml_attr(&measured_box.slot),
    ))
}

fn paragraph_xml(paragraph: &MeasuredParagraph) -> Result<String> {
    let align = paragraph_align(&paragraph.align);
    let (bullet_attrs, bullet) = match paragraph.bullet_level {
        Some(level) => bullet_xml(level, paragraph.numbered),
        None => (String::new(), String::new()),
    };
    let runs = paragraph
        .runs
        .iter()
        .map(run_xml)
        .collect::<Result<String>>()?;
    Ok(format!(
        r#"<a:p><a:pPr{bullet_attrs} algn="{align}">{bullet}</a:pPr>{runs}</a:p>"#
    ))
}

fn bullet_xml(level: u8, numbered: bool) -> (String, String) {
    let margin = 342_900 * (i64::from(level) + 1);
    let indent = -171_450;
    let bullet = if numbered {
        r#"<a:buFont typeface="Arial"/><a:buAutoNum type="arabicPeriod"/>"#
    } else {
        r#"<a:buFont typeface="Arial"/><a:buChar char="&#8226;"/>"#
    };
    (
        format!(r#" marL="{margin}" indent="{indent}""#),
        bullet.to_owned(),
    )
}

fn run_xml(run: &MeasuredRun) -> Result<String> {
    let size = (run.font_size_px * 75.0).round() as i64;
    let bold = if run.bold { r#" b="1""# } else { "" };
    let italic = if run.italic { r#" i="1""# } else { "" };
    let underline = if run.underline { r#" u="sng""# } else { "" };
    let color = run_color_xml(&run.color)?;
    Ok(format!(
        r#"<a:r><a:rPr lang="en-US" sz="{size}"{bold}{italic}{underline}>{color}<a:latin typeface="{font}"/></a:rPr><a:t>{text}</a:t></a:r>"#,
        font = xml_attr(&run.font_family),
        text = xml_text(&run.text),
    ))
}

fn picture_xml(shape_id: usize, image: &MeasuredImage, rel_id: &str) -> String {
    let rect = xfrm_xml(&image.rect);
    format!(
        r#"<p:pic><p:nvPicPr><p:cNvPr id="{shape_id}" name="Picture {shape_id}" descr="{descr}"/><p:cNvPicPr><a:picLocks noChangeAspect="1"/></p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="{rel_id}"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr>{rect}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic>"#,
        descr = xml_attr(&image.alt),
    )
}

fn slide_background_xml(color: &str) -> Result<String> {
    Ok(match solid_fill_xml(color)? {
        Some(fill) => format!(r#"<p:bg><p:bgPr>{fill}</p:bgPr></p:bg>"#),
        None => String::new(),
    })
}

fn xfrm_xml(rect: &MeasuredRect) -> String {
    format!(
        r#"<a:xfrm><a:off x="{}" y="{}"/><a:ext cx="{}" cy="{}"/></a:xfrm>"#,
        px_to_emu(rect.x),
        px_to_emu(rect.y),
        px_to_emu(rect.w),
        px_to_emu(rect.h)
    )
}

fn fill_xml(color: &str) -> Result<String> {
    Ok(solid_fill_xml(color)?.unwrap_or_else(|| "<a:noFill/>".to_owned()))
}

fn line_xml(color: &str, width_px: f64) -> Result<String> {
    if width_px <= 0.0 {
        return Ok(r#"<a:ln><a:noFill/></a:ln>"#.to_owned());
    }
    Ok(match solid_fill_xml(color)? {
        Some(fill) => format!(r#"<a:ln w="{}">{fill}</a:ln>"#, px_to_emu(width_px)),
        None => r#"<a:ln><a:noFill/></a:ln>"#.to_owned(),
    })
}

fn run_color_xml(color: &str) -> Result<String> {
    Ok(solid_fill_xml(color)?.unwrap_or_else(|| "<a:noFill/>".to_owned()))
}

fn solid_fill_xml(color: &str) -> Result<Option<String>> {
    let color = parse_css_color(color)?;
    if color.alpha <= 0.0 {
        return Ok(None);
    }
    let srgb = if color.alpha >= 1.0 {
        format!(r#"<a:srgbClr val="{}"/>"#, color.hex)
    } else {
        format!(
            r#"<a:srgbClr val="{}"><a:alpha val="{}"/></a:srgbClr>"#,
            color.hex,
            alpha_to_ooxml(color.alpha)
        )
    };
    Ok(Some(format!(r#"<a:solidFill>{srgb}</a:solidFill>"#)))
}

#[derive(Debug)]
struct CssColor {
    hex: String,
    alpha: f64,
}

fn parse_css_color(raw: &str) -> Result<CssColor> {
    let raw = raw.trim();
    if raw.eq_ignore_ascii_case("transparent") {
        return Ok(CssColor {
            hex: "000000".to_owned(),
            alpha: 0.0,
        });
    }
    if let Some(hex) = raw.strip_prefix('#') {
        if hex.len() == 6 && hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return Ok(CssColor {
                hex: hex.to_ascii_uppercase(),
                alpha: 1.0,
            });
        }
    }
    let args = raw
        .strip_prefix("rgb(")
        .and_then(|s| s.strip_suffix(')'))
        .map(|s| (s, None))
        .or_else(|| {
            raw.strip_prefix("rgba(")
                .and_then(|s| s.strip_suffix(')'))
                .map(|s| (s, Some(())))
        })
        .ok_or_else(|| unsupported_color_error(raw))?;
    let parts = args.0.split(',').map(str::trim).collect::<Vec<_>>();
    if !(3..=4).contains(&parts.len()) {
        return Err(unsupported_color_error(raw));
    }
    let r = parse_color_channel(parts[0]).ok_or_else(|| unsupported_color_error(raw))?;
    let g = parse_color_channel(parts[1]).ok_or_else(|| unsupported_color_error(raw))?;
    let b = parse_color_channel(parts[2]).ok_or_else(|| unsupported_color_error(raw))?;
    let alpha = parts
        .get(3)
        .map(|alpha| parse_alpha_channel(alpha).ok_or_else(|| unsupported_color_error(raw)))
        .transpose()?
        .unwrap_or(1.0);
    Ok(CssColor {
        hex: format!("{r:02X}{g:02X}{b:02X}"),
        alpha,
    })
}

fn unsupported_color_error(raw: &str) -> BuildError {
    pptx_error(
        format!("unsupported color value '{raw}'"),
        "rerun measurement in Chrome so CSS colors are normalized before pptx export",
    )
}

fn parse_color_channel(raw: &str) -> Option<u8> {
    let value = raw.parse::<f64>().ok()?.round();
    (0.0..=255.0).contains(&value).then_some(value as u8)
}

fn parse_alpha_channel(raw: &str) -> Option<f64> {
    let raw = raw.trim();
    let value = if let Some(percent) = raw.strip_suffix('%') {
        percent.parse::<f64>().ok()? / 100.0
    } else {
        raw.parse::<f64>().ok()?
    };
    (0.0..=1.0).contains(&value).then_some(value)
}

fn alpha_to_ooxml(alpha: f64) -> i64 {
    (alpha * 100_000.0).round() as i64
}

fn paragraph_align(raw: &str) -> &'static str {
    match raw {
        "center" | "centre" => "ctr",
        "right" | "end" => "r",
        "justify" => "just",
        _ => "l",
    }
}

fn media_extension(src: &str, source: &Path) -> Result<String> {
    let extension = Path::new(src)
        .extension()
        .or_else(|| source.extension())
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .ok_or_else(|| {
            pptx_error(
                format!("image path {src} has no file extension"),
                "use image assets with png, jpg, jpeg, gif, or webp extensions",
            )
        })?;
    Ok(extension)
}

fn image_content_type(extension: &str) -> Result<&'static str> {
    match extension {
        "png" => Ok("image/png"),
        "jpg" | "jpeg" => Ok("image/jpeg"),
        "gif" => Ok("image/gif"),
        "webp" => Ok("image/webp"),
        other => Err(pptx_error(
            format!("unsupported pptx image extension: {other}"),
            "use png, jpg, jpeg, gif, or webp images",
        )),
    }
}

fn slide_master_xml() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldMaster xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr></p:spTree></p:cSld><p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/><p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst><p:txStyles><p:titleStyle/><p:bodyStyle/><p:otherStyle/></p:txStyles></p:sldMaster>"#.to_owned()
}

fn slide_master_rels_xml() -> String {
    relationships_xml(
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="../theme/theme1.xml"/>"#,
    )
}

fn slide_layout_xml() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldLayout xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" type="blank" preserve="1"><p:cSld name="Blank"><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"#.to_owned()
}

fn slide_layout_rels_xml() -> String {
    relationships_xml(
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/>"#,
    )
}

fn notes_master_xml() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:notesMaster xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr></p:spTree></p:cSld><p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/></p:notesMaster>"#.to_owned()
}

fn notes_master_rels_xml() -> String {
    relationships_xml(
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="../theme/theme1.xml"/>"#,
    )
}

fn notes_slide_xml(slide_number: usize, notes: &str) -> Result<String> {
    let rect = MeasuredRect {
        x: 48.0,
        y: 360.0,
        w: 624.0,
        h: 288.0,
    };
    let paragraphs = notes_paragraphs_xml(notes)?;
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:notes xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree>{group}<p:sp><p:nvSpPr><p:cNvPr id="2" name="Notes Placeholder {slide_number}"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr><p:spPr>{rect}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/><a:ln><a:noFill/></a:ln></p:spPr><p:txBody><a:bodyPr wrap="square"><a:noAutofit/></a:bodyPr><a:lstStyle/>{paragraphs}</p:txBody></p:sp></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:notes>"#,
        group = group_shape_xml(),
        rect = xfrm_xml(&rect),
    ))
}

fn notes_paragraphs_xml(notes: &str) -> Result<String> {
    notes.split('\n').map(note_paragraph_xml).collect()
}

fn note_paragraph_xml(line: &str) -> Result<String> {
    let runs = if line.is_empty() {
        Vec::new()
    } else {
        vec![MeasuredRun {
            text: line.to_owned(),
            color: "rgb(0, 0, 0)".to_owned(),
            font_family: "Arial".to_owned(),
            font_size_px: 16.0,
            bold: false,
            italic: false,
            underline: false,
        }]
    };
    paragraph_xml(&MeasuredParagraph {
        align: "left".to_owned(),
        bullet_level: None,
        numbered: false,
        runs,
    })
}

fn notes_slide_rels_xml(slide_number: usize) -> String {
    relationships_xml(&format!(
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="../slides/slide{slide_number}.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesMaster" Target="../notesMasters/notesMaster1.xml"/>"#
    ))
}

fn theme_xml() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Peitho"><a:themeElements><a:clrScheme name="Peitho"><a:dk1><a:srgbClr val="000000"/></a:dk1><a:lt1><a:srgbClr val="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="1F2937"/></a:dk2><a:lt2><a:srgbClr val="F8FAFC"/></a:lt2><a:accent1><a:srgbClr val="2563EB"/></a:accent1><a:accent2><a:srgbClr val="16A34A"/></a:accent2><a:accent3><a:srgbClr val="DC2626"/></a:accent3><a:accent4><a:srgbClr val="7C3AED"/></a:accent4><a:accent5><a:srgbClr val="0891B2"/></a:accent5><a:accent6><a:srgbClr val="CA8A04"/></a:accent6><a:hlink><a:srgbClr val="2563EB"/></a:hlink><a:folHlink><a:srgbClr val="7C3AED"/></a:folHlink></a:clrScheme><a:fontScheme name="Peitho"><a:majorFont><a:latin typeface="Arial"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="Arial"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme><a:fmtScheme name="Peitho"><a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:fillStyleLst><a:lnStyleLst><a:ln w="9525"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/></a:ln><a:ln w="25400"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/></a:ln><a:ln w="38100"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/></a:ln></a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>"#.to_owned()
}

fn relationships_xml(inner: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{inner}</Relationships>"#
    )
}

fn group_shape_xml() -> &'static str {
    r#"<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>"#
}

fn px_to_emu(px: f64) -> i64 {
    (px * EMU_PER_PX).round() as i64
}

fn xml_text(raw: &str) -> String {
    encode_text(raw).into_owned()
}

fn xml_attr(raw: &str) -> String {
    encode_double_quoted_attribute(raw).into_owned()
}

fn pptx_io_error(err: impl std::fmt::Display) -> BuildError {
    pptx_error(
        format!("failed to write pptx: {err}"),
        "retry export and check filesystem permissions",
    )
}

fn pptx_error(message: impl Into<String>, help: impl Into<String>) -> BuildError {
    BuildError::new(ErrorKind::Manifest, None, message, help)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::{Cursor, Read},
        path::Path,
    };

    use zip::{CompressionMethod, ZipArchive};

    use crate::{
        domain::{
            MeasuredBox, MeasuredBoxStyle, MeasuredDeck, MeasuredImage, MeasuredParagraph,
            MeasuredRect, MeasuredRun, MeasuredSlide, RenderedSlide, ResolvedImageAsset,
            ResolvedImagePath, SlideKey,
        },
        phase::{Deck, DeckSettings},
        Rendered,
    };

    #[test]
    fn pptx_writer_emits_required_parts_text_images_notes_and_standard_size() {
        let dir = tempfile::tempdir().unwrap();
        let image_path = dir.path().join("arch.png");
        fs::write(&image_path, b"png bytes").unwrap();
        let image_asset = image_asset(&image_path, "arch.png");
        let measured = measured_deck_with_notes_image(image_asset.dist_rel.as_str());
        let deck = rendered_deck(vec![("intro", Some("speaker notes <private>"))]);

        let bytes = super::build_pptx(&measured, &deck, &[image_asset]).unwrap();
        let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();

        assert!(zip.by_name("[Content_Types].xml").is_ok());
        assert!(zip.by_name("_rels/.rels").is_ok());
        assert!(zip.by_name("docProps/core.xml").is_ok());
        assert!(zip.by_name("docProps/app.xml").is_ok());
        assert!(zip.by_name("ppt/presentation.xml").is_ok());
        assert!(zip.by_name("ppt/slideMasters/slideMaster1.xml").is_ok());
        assert!(zip.by_name("ppt/slideLayouts/slideLayout1.xml").is_ok());
        assert!(zip.by_name("ppt/theme/theme1.xml").is_ok());
        assert!(zip.by_name("ppt/notesMasters/notesMaster1.xml").is_ok());
        assert!(zip.by_name("ppt/notesSlides/notesSlide1.xml").is_ok());
        assert_eq!(read_zip(&mut zip, "ppt/media/image1.png"), "png bytes");

        let presentation = read_zip(&mut zip, "ppt/presentation.xml");
        assert!(presentation.contains(r#"<p:sldSz cx="12192000" cy="6858000"/>"#));
        assert!(!presentation.contains("type=\"wide\""));
        assert!(!presentation.contains("type=\"screen4x3\""));
        assert!(presentation.contains(
            r#"<p:notesMasterIdLst><p:notesMasterId r:id="rId3"/></p:notesMasterIdLst><p:sldIdLst>"#
        ));
        assert!(presentation.contains(r#"r:id="rId2""#));

        let slide = read_zip(&mut zip, "ppt/slides/slide1.xml");
        assert!(slide.contains(r#"<a:srgbClr val="F8F8F8"/>"#));
        assert!(slide.contains(r#"<a:off x="914400" y="762000"/>"#));
        assert!(slide.contains(r#"<a:ext cx="6096000" cy="1143000"/>"#));
        assert!(slide.contains(r#"<a:t>Hello &amp; welcome</a:t>"#));
        assert!(slide.contains(r#"sz="4200""#));
        assert!(slide.contains(r#"b="1""#));
        assert!(slide.contains(r#"u="sng""#));
        assert!(slide.contains(r#"<a:srgbClr val="112233"/>"#));
        assert!(slide.contains(r#"<a:latin typeface="Inter"/>"#));
        assert!(slide.contains(
            r#"<a:bodyPr wrap="square" anchor="t" lIns="0" tIns="0" rIns="0" bIns="0"><a:noAutofit/></a:bodyPr>"#
        ));
        assert!(slide.contains(
            r#"<a:pPr marL="342900" indent="-171450" algn="ctr"><a:buFont typeface="Arial"/><a:buChar char="&#8226;"/></a:pPr>"#
        ));
        assert!(!slide.contains("<a:marL"));
        assert!(!slide.contains("<a:indent"));
        assert!(slide.contains(r#"descr="Architecture""#));
        assert!(slide.contains(r#"<a:blip r:embed="rId2"/>"#));

        let notes = read_zip(&mut zip, "ppt/notesSlides/notesSlide1.xml");
        assert!(notes.contains("speaker notes &lt;private&gt;"));
        assert!(notes.contains(r#"<a:off x="457200" y="3429000"/>"#));
        assert!(notes.contains(r#"<a:ext cx="5943600" cy="2743200"/>"#));
        assert!(notes.contains(r#"<p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr>"#));
        assert!(!notes.contains(r#"<p:cNvSpPr txBox="1"/>"#));

        let slide_rels = read_zip(&mut zip, "ppt/slides/_rels/slide1.xml.rels");
        assert!(slide_rels.contains(r#"Target="../slideLayouts/slideLayout1.xml""#));
        assert!(slide_rels.contains(r#"Target="../media/image1.png""#));
    }

    #[test]
    fn pptx_writer_emits_numbered_list_paragraphs() {
        let mut measured = measured_deck_without_images("intro");
        measured.slides[0].boxes[0].paragraphs[0].numbered = true;
        let deck = rendered_deck(vec![("intro", None)]);

        let bytes = super::build_pptx(&measured, &deck, &[]).unwrap();
        let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();

        let slide = read_zip(&mut zip, "ppt/slides/slide1.xml");
        assert!(slide.contains(
            r#"<a:pPr marL="342900" indent="-171450" algn="ctr"><a:buFont typeface="Arial"/><a:buAutoNum type="arabicPeriod"/></a:pPr>"#
        ));
        assert!(!slide.contains(r#"<a:buChar char="&#8226;"/>"#));
    }

    #[test]
    fn pptx_writer_splits_multiline_notes_into_ooxml_paragraphs() {
        let measured = measured_deck_without_images("intro");
        let deck = rendered_deck(vec![("intro", Some("first paragraph\n\nsecond paragraph"))]);

        let bytes = super::build_pptx(&measured, &deck, &[]).unwrap();
        let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();

        let notes = read_zip(&mut zip, "ppt/notesSlides/notesSlide1.xml");
        assert!(notes.contains("<a:t>first paragraph</a:t>"));
        assert!(notes.contains(r#"<a:p><a:pPr algn="l"></a:pPr></a:p>"#));
        assert!(notes.contains("<a:t>second paragraph</a:t>"));
        assert!(!notes.contains("first paragraph\n\nsecond paragraph"));
    }

    #[test]
    fn pptx_writer_omits_notes_parts_when_no_slides_have_notes() {
        let measured = measured_deck_without_images("intro");
        let deck = rendered_deck(vec![("intro", None)]);

        let bytes = super::build_pptx(&measured, &deck, &[]).unwrap();
        let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();

        assert!(zip.by_name("ppt/notesMasters/notesMaster1.xml").is_err());
        assert!(zip.by_name("ppt/notesSlides/notesSlide1.xml").is_err());
        let content_types = read_zip(&mut zip, "[Content_Types].xml");
        assert!(!content_types.contains("notesSlides"));
    }

    #[test]
    fn pptx_writer_rejects_measured_slide_count_mismatch() {
        let measured = measured_deck_without_images("intro");
        let deck = rendered_deck(vec![("intro", None), ("details", None)]);

        let err = super::build_pptx(&measured, &deck, &[]).unwrap_err();

        assert!(err
            .to_string()
            .contains("measured slide count 1 does not match rendered slide count 2"));
    }

    #[test]
    fn pptx_writer_rejects_measured_image_without_resolved_asset() {
        let measured = measured_deck_with_notes_image("assets/0123456789abcdef-missing.png");
        let deck = rendered_deck(vec![("intro", None)]);

        let err = super::build_pptx(&measured, &deck, &[]).unwrap_err();

        assert!(err.to_string().contains(
            "measured image assets/0123456789abcdef-missing.png has no resolved image asset"
        ));
    }

    #[test]
    fn pptx_writer_rejects_unsupported_color_values() {
        let mut measured = measured_deck_without_images("intro");
        measured.slides[0].boxes[0].paragraphs[0].runs[0].color = "oklch(50% 0.1 120)".to_owned();
        let deck = rendered_deck(vec![("intro", None)]);

        let err = super::build_pptx(&measured, &deck, &[]).unwrap_err();

        assert!(err
            .to_string()
            .contains("unsupported color value 'oklch(50% 0.1 120)'"));
    }

    #[test]
    fn solid_fill_xml_preserves_fractional_alpha() {
        let transparent_fill = super::solid_fill_xml("rgba(0, 0, 0, 0.08)")
            .unwrap()
            .unwrap();
        assert!(transparent_fill
            .contains(r#"<a:srgbClr val="000000"><a:alpha val="8000"/></a:srgbClr>"#));

        let opaque_fill = super::solid_fill_xml("rgb(0, 0, 0)").unwrap().unwrap();
        assert!(opaque_fill.contains(r#"<a:srgbClr val="000000"/>"#));
        assert!(!opaque_fill.contains("<a:alpha"));
    }

    #[test]
    fn theme_xml_emits_schema_minimum_format_scheme_and_font_children() {
        let theme = super::theme_xml();
        let fill_styles = between(&theme, "<a:fillStyleLst>", "</a:fillStyleLst>");
        let line_styles = between(&theme, "<a:lnStyleLst>", "</a:lnStyleLst>");
        let effect_styles = between(&theme, "<a:effectStyleLst>", "</a:effectStyleLst>");
        let background_fill_styles = between(&theme, "<a:bgFillStyleLst>", "</a:bgFillStyleLst>");

        assert_eq!(fill_styles.matches("<a:solidFill>").count(), 3);
        assert_eq!(line_styles.matches("<a:ln ").count(), 3);
        assert_eq!(effect_styles.matches("<a:effectStyle>").count(), 3);
        assert_eq!(effect_styles.matches("<a:effectLst/>").count(), 3);
        assert_eq!(background_fill_styles.matches("<a:solidFill>").count(), 3);
        assert!(theme.contains(
            r#"<a:majorFont><a:latin typeface="Arial"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont>"#
        ));
        assert!(theme.contains(
            r#"<a:minorFont><a:latin typeface="Arial"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont>"#
        ));
    }

    #[test]
    fn pptx_writer_deduplicates_media_by_source_and_deflates_xml_parts() {
        let dir = tempfile::tempdir().unwrap();
        let image_path = dir.path().join("arch.png");
        fs::write(&image_path, b"png bytes").unwrap();
        let image_asset = image_asset(&image_path, "arch.png");
        let mut measured = measured_deck_with_notes_image(image_asset.dist_rel.as_str());
        let mut second_slide = measured.slides[0].clone();
        second_slide.key = "details".to_owned();
        measured.slides.push(second_slide);
        let deck = rendered_deck(vec![("intro", None), ("details", None)]);

        let bytes = super::build_pptx(&measured, &deck, &[image_asset]).unwrap();
        let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();
        let media_names = (0..zip.len())
            .map(|index| zip.by_index(index).unwrap().name().to_owned())
            .filter(|name| name.starts_with("ppt/media/"))
            .collect::<Vec<_>>();

        assert_eq!(media_names, vec!["ppt/media/image1.png"]);
        assert_eq!(
            zip.by_name("ppt/slides/slide1.xml").unwrap().compression(),
            CompressionMethod::Deflated
        );
        assert_eq!(
            zip.by_name("ppt/media/image1.png").unwrap().compression(),
            CompressionMethod::Stored
        );
        let slide1_rels = read_zip(&mut zip, "ppt/slides/_rels/slide1.xml.rels");
        let slide2_rels = read_zip(&mut zip, "ppt/slides/_rels/slide2.xml.rels");
        assert!(slide1_rels.contains(r#"Target="../media/image1.png""#));
        assert!(slide2_rels.contains(r#"Target="../media/image1.png""#));
    }

    fn measured_deck_with_notes_image(src: &str) -> MeasuredDeck {
        MeasuredDeck {
            canvas_width: 1280.0,
            canvas_height: 720.0,
            slides: vec![MeasuredSlide {
                key: "intro".to_owned(),
                background_color: "rgb(248, 248, 248)".to_owned(),
                boxes: vec![MeasuredBox {
                    slot: "title".to_owned(),
                    rect: MeasuredRect {
                        x: 96.0,
                        y: 80.0,
                        w: 640.0,
                        h: 120.0,
                    },
                    style: MeasuredBoxStyle {
                        background_color: "rgba(0, 0, 0, 0)".to_owned(),
                        border_color: "rgb(80, 90, 100)".to_owned(),
                        border_width: 2.0,
                        border_radius: 8.0,
                    },
                    paragraphs: vec![MeasuredParagraph {
                        align: "center".to_owned(),
                        bullet_level: Some(0),
                        numbered: false,
                        runs: vec![MeasuredRun {
                            text: "Hello & welcome".to_owned(),
                            color: "rgb(17, 34, 51)".to_owned(),
                            font_family: "Inter".to_owned(),
                            font_size_px: 56.0,
                            bold: true,
                            italic: false,
                            underline: true,
                        }],
                    }],
                }],
                images: vec![MeasuredImage {
                    src: src.to_owned(),
                    alt: "Architecture".to_owned(),
                    rect: MeasuredRect {
                        x: 800.0,
                        y: 180.0,
                        w: 320.0,
                        h: 180.0,
                    },
                }],
            }],
        }
    }

    fn measured_deck_without_images(key: &str) -> MeasuredDeck {
        let mut measured = measured_deck_with_notes_image("assets/0123456789abcdef-arch.png");
        measured.slides[0].key = key.to_owned();
        measured.slides[0].images.clear();
        measured
    }

    fn rendered_deck(slides: Vec<(&str, Option<&str>)>) -> Deck<Rendered> {
        let rendered = slides
            .into_iter()
            .enumerate()
            .map(|(index, (key, notes))| {
                RenderedSlide::new(
                    index,
                    SlideKey::new(key).unwrap(),
                    String::new(),
                    notes.map(str::to_owned),
                )
            })
            .collect();
        Deck::rendered(DeckSettings::default(), rendered, String::new())
    }

    fn image_asset(source_abs: &Path, basename: &str) -> ResolvedImageAsset {
        ResolvedImageAsset {
            source_abs: source_abs.to_path_buf(),
            dist_rel: ResolvedImagePath::from_hashed_asset("0123456789abcdef", basename).unwrap(),
        }
    }

    fn read_zip(zip: &mut ZipArchive<Cursor<Vec<u8>>>, path: &str) -> String {
        let mut file = zip.by_name(path).unwrap();
        let mut content = String::new();
        file.read_to_string(&mut content).unwrap();
        content
    }

    fn between<'a>(haystack: &'a str, start: &str, end: &str) -> &'a str {
        let after_start = haystack.split_once(start).unwrap().1;
        after_start.split_once(end).unwrap().0
    }
}
