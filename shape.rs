use fontcode::cmap::{Cmap, CmapSubtable};
use fontcode::error::{ParseError, ShapingError};
use fontcode::glyph_index::read_cmap_subtable;
use fontcode::gsub::{gsub_apply_default, GlyphOrigin, RawGlyph};
use fontcode::layout::{GDEFTable, LayoutTable, LayoutTableType};
use fontcode::read::ReadScope;
use fontcode::tables::{OffsetTable, OpenTypeFile, OpenTypeFont, TTCHeader};
use fontcode::tag;
use std::env;
use std::fs::File;
use std::io::{self, Read};

fn main() -> Result<(), ShapingError> {
    let args: Vec<String> = env::args().collect();

    if args.len() != 5 {
        println!("Usage: shape FONTFILE SCRIPT LANG TEXT");
        return Ok(());
    }

    let filename = &args[1];
    let script = tag_from_string(&args[2])?;
    let lang = tag_from_string(&args[3])?;
    let text = &args[4];
    let buffer = read_file(filename)?;

    let fontfile = ReadScope::new(&buffer).read::<OpenTypeFile>()?;

    match fontfile.font {
        OpenTypeFont::Single(ttf) => shape_ttf(fontfile.scope, ttf, script, lang, text)?,
        OpenTypeFont::Collection(ttc) => shape_ttc(fontfile.scope, ttc, script, lang, text)?,
    }

    Ok(())
}

fn read_file(path: &str) -> Result<Vec<u8>, io::Error> {
    let mut file = File::open(path)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    Ok(buffer)
}

fn shape_ttc<'a>(
    scope: ReadScope<'a>,
    ttc: TTCHeader<'a>,
    script: u32,
    lang: u32,
    text: &str,
) -> Result<(), ShapingError> {
    for offset_table_offset in &ttc.offset_tables {
        let offset_table_offset = offset_table_offset as usize; // FIXME range
        let offset_table = scope.offset(offset_table_offset).read::<OffsetTable>()?;
        shape_ttf(scope, offset_table, script, lang, text)?;
    }
    Ok(())
}

fn shape_ttf<'a>(
    scope: ReadScope<'a>,
    ttf: OffsetTable<'a>,
    script: u32,
    lang: u32,
    text: &str,
) -> Result<(), ShapingError> {
    let cmap = if let Some(cmap_scope) = ttf.read_table(scope, tag::CMAP)? {
        cmap_scope.read::<Cmap>()?
    } else {
        println!("no cmap table");
        return Ok(());
    };
    let cmap_subtable = if let Some(cmap_subtable) = read_cmap_subtable(&cmap)? {
        cmap_subtable
    } else {
        println!("no suitable cmap subtable");
        return Ok(());
    };
    let opt_glyphs_res: Result<Vec<_>, _> = text
        .chars()
        .map(|ch| map_glyph(&cmap_subtable, ch))
        .collect();
    let opt_glyphs = opt_glyphs_res?;
    let mut glyphs = opt_glyphs.into_iter().flatten().collect();
    println!("glyphs before: {:?}", glyphs);
    if let Some(gsub_record) = ttf.find_table_record(tag::GSUB) {
        let gsub_table_data = gsub_record.read_table(scope)?.data();
        let opt_gdef_table_data = match ttf.find_table_record(tag::GDEF) {
            Some(gdef_record) => Some(gdef_record.read_table(scope)?.data()),
            None => None,
        };
        let vertical = false;
        let res = with_tables(
            gsub_table_data,
            opt_gdef_table_data,
            |gsub_table, opt_gdef_table| {
                gsub_apply_default(
                    &|| make_dotted_circle(&cmap_subtable),
                    &gsub_table,
                    opt_gdef_table,
                    script,
                    lang,
                    vertical,
                    &mut glyphs,
                )
            },
        )?;
        println!("res: {}", res);
        if res {
            println!("glyphs after: {:?}", glyphs);
        }
    } else {
        println!("no GSUB table");
    }
    Ok(())
}

fn with_tables<T: LayoutTableType, Ret>(
    layout_table_data: &[u8],
    opt_gdef_table_data: Option<&[u8]>,
    f: impl FnOnce(&LayoutTable<T>, Option<&GDEFTable>) -> Result<Ret, ShapingError>,
) -> Result<Ret, ShapingError> {
    let layout_table = ReadScope::new(layout_table_data).read::<LayoutTable<T>>()?;
    match opt_gdef_table_data {
        Some(gdef_table_data) => {
            let gdef_table = ReadScope::new(gdef_table_data).read::<GDEFTable>()?;
            f(&layout_table, Some(&gdef_table))
        }
        None => f(&layout_table, None),
    }
}

fn make_dotted_circle(cmap_subtable: &CmapSubtable) -> Vec<RawGlyph<()>> {
    match map_glyph(cmap_subtable, '\u{25cc}') {
        Ok(Some(raw_glyph)) => vec![raw_glyph],
        _ => Vec::new(),
    }
}

fn map_glyph(cmap_subtable: &CmapSubtable, ch: char) -> Result<Option<RawGlyph<()>>, ParseError> {
    if let Some(glyph_index) = cmap_subtable.map_glyph(ch as u32)? {
        let glyph = make_glyph(ch, glyph_index);
        Ok(Some(glyph))
    } else {
        Ok(None)
    }
}

fn make_glyph(ch: char, glyph_index: u16) -> RawGlyph<()> {
    RawGlyph {
        unicodes: vec![ch],
        glyph_index: Some(glyph_index),
        liga_component_pos: 0,
        glyph_origin: GlyphOrigin::Char(ch),
        small_caps: false,
        multi_subst_dup: false,
        is_vert_alt: false,
        fake_bold: false,
        fake_italic: false,
        extra_data: (),
    }
}

fn tag_from_string(s: &str) -> Result<u32, ParseError> {
    if s.len() > 4 {
        return Err(ParseError::BadValue);
    }

    let mut tag: u32 = 0;
    let mut count = 0;

    for c in s.chars() {
        if !c.is_ascii() || c.is_ascii_control() {
            return Err(ParseError::BadValue);
        }

        tag = (tag << 8) | (c as u32);
        count += 1;
    }

    while count < 4 {
        tag = (tag << 8) | (' ' as u32);
    }

    Ok(tag)
}
