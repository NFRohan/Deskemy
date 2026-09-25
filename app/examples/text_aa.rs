//! Compare Skia text rendering modes for the UI font, offscreen: what Slint's
//! Skia renderer does today (grayscale, subpixel positioning) against
//! ClearType-style subpixel (LCD) antialiasing. Writes a PNG.
//!
//! `cargo run --example text_aa -- out.png`

use skia_safe::font::Edging;
use skia_safe::font_arguments::variation_position::Coordinate;
use skia_safe::font_arguments::VariationPosition;
use skia_safe::{
    surfaces, Color, EncodedImageFormat, Font, FontArguments, FontHinting, FontMgr, FourByteTag, ImageInfo,
    Paint, PixelGeometry, SurfaceProps, SurfacePropsFlags,
};

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "text_aa.png".into());
    let data = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/ui/fonts/InterVariable.ttf")).unwrap();
    let base = FontMgr::new().new_from_data(skia_safe::Data::new_copy(&data), None).expect("Inter");
    let weight = |w: f32| {
        let coords = [Coordinate { axis: FourByteTag::from_chars('w', 'g', 'h', 't'), value: w }];
        let args = FontArguments::new().set_variation_design_position(VariationPosition { coordinates: &coords });
        base.clone_with_arguments(&args).unwrap()
    };

    let modes: [(&str, Edging, FontHinting); 4] = [
        ("today: grayscale", Edging::AntiAlias, FontHinting::Normal),
        ("grayscale + slight hinting", Edging::AntiAlias, FontHinting::Slight),
        ("LCD subpixel", Edging::SubpixelAntiAlias, FontHinting::Normal),
        ("LCD subpixel + slight hinting", Edging::SubpixelAntiAlias, FontHinting::Slight),
    ];
    let (w, h) = (760, 60 + modes.len() as i32 * 96);
    let props = SurfaceProps::new(SurfacePropsFlags::default(), PixelGeometry::RGBH);
    let mut surface = surfaces::raster(&ImageInfo::new_n32_premul((w, h), None), None, Some(&props)).unwrap();
    let canvas = surface.canvas();
    canvas.clear(Color::from_rgb(0x11, 0x13, 0x17));

    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    let mut y = 30.0;
    for (label, edging, hinting) in modes {
        paint.set_color(Color::from_rgb(0x8c, 0x90, 0x9a));
        let mut small = Font::from_typeface(weight(400.0), 11.0);
        small.set_edging(Edging::AntiAlias);
        canvas.draw_str(label, (16.0, y), &small, &paint);
        y += 26.0;
        for (size, wght, text) in [
            (16.0, 600.0, "Udemy - Linux Administration Bootcamp Go from Beginner"),
            (13.0, 400.0, "Ultimate AWS Certified Solutions Architect · 385 Lectures · Continue where you left off"),
        ] {
            let mut font = Font::from_typeface(weight(wght), size);
            font.set_subpixel(true);
            font.set_edging(edging);
            font.set_hinting(hinting);
            paint.set_color(Color::from_rgb(0xe2, 0xe2, 0xe8));
            canvas.draw_str(text, (16.0, y), &font, &paint);
            y += 26.0;
        }
        y += 18.0;
    }

    let png = surface.image_snapshot().encode(None, EncodedImageFormat::PNG, None).unwrap();
    std::fs::write(&out, png.as_bytes()).unwrap();
    println!("wrote {out}");
}
