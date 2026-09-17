//! Probe: decode metadata, preview, and full pixels for a RAW file.
//! Usage: cargo run -p laika-raw --example probe fixtures/raw/IMG_5442.NEF

use std::time::Instant;

fn main() {
    let path = std::env::args().nth(1).expect("path to raw file");
    let path = std::path::Path::new(&path);

    let t = Instant::now();
    let review = laika_raw::preview::review_jpeg(path, laika_raw::preview::PREVIEW_SMALL)
        .ok()
        .and_then(|jpeg| image::load_from_memory(&jpeg).ok());
    println!(
        "review: {:?} -> {:?}",
        t.elapsed(),
        review.map(|image| (image.width(), image.height()))
    );

    let t = Instant::now();
    let derivatives =
        laika_raw::preview::import_derivatives(path)
            .ok()
            .and_then(|(small, large)| {
                let small = image::load_from_memory(&small).ok()?;
                let large = image::load_from_memory(&large).ok()?;
                Some((
                    (small.width(), small.height()),
                    (large.width(), large.height()),
                ))
            });
    println!("import derivatives: {:?} -> {:?}", t.elapsed(), derivatives);

    let t = Instant::now();
    let raw = rawler::decode_file(path).expect("decode_file");
    println!("decode_file: {:?}", t.elapsed());
    println!(
        "camera: {} {} (clean: {} {})",
        raw.make, raw.model, raw.clean_make, raw.clean_model
    );
    println!(
        "size: {}x{} cpp={} bps={}",
        raw.width, raw.height, raw.cpp, raw.bps
    );
    println!("wb: {:?}", raw.wb_coeffs);
    println!("black: {:?} white: {:?}", raw.blacklevel, raw.whitelevel);
    println!("orientation: {:?}", raw.orientation);
    println!(
        "active_area: {:?} crop_area: {:?}",
        raw.active_area, raw.crop_area
    );

    let t = Instant::now();
    let params = rawler::decoders::RawDecodeParams::default();
    let thumb = rawler::analyze::extract_thumbnail_pixels(path, &params).ok();
    println!(
        "thumbnail: {:?} -> {:?}",
        t.elapsed(),
        thumb.map(|t| (t.width(), t.height()))
    );

    let t = Instant::now();
    let preview = rawler::analyze::extract_preview_pixels(path, &params).ok();
    println!(
        "preview: {:?} -> {:?}",
        t.elapsed(),
        preview.map(|t| (t.width(), t.height()))
    );

    let t = Instant::now();
    let dev = rawler::imgop::develop::RawDevelop::default();
    let developed = dev.develop_intermediate(&raw).expect("develop");
    println!("develop_intermediate: {:?}", t.elapsed());
    let dynimg = developed.to_dynamic_image().expect("to_dynamic_image");
    println!("developed: {}x{}", dynimg.width(), dynimg.height());
}
