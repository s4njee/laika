//! Render a RAW file through the develop pipeline and save PNGs.
//! Usage: cargo run -p laika-develop --release --example render -- fixtures/raw/IMG_5442.NEF /tmp/laika-dev

use std::time::Instant;

fn save(out: &str, name: &str, frame: &laika_develop::Rendered) {
    let buf: image::ImageBuffer<image::Rgba<u8>, Vec<u8>> =
        image::ImageBuffer::from_raw(frame.width, frame.height, frame.rgba.clone()).unwrap();
    buf.save(format!("{out}/{name}.png")).unwrap();
}

fn main() {
    let path = std::env::args().nth(1).expect("raw path");
    let out = std::env::args().nth(2).expect("out dir");
    std::fs::create_dir_all(&out).unwrap();

    let t = Instant::now();
    let img = laika_raw::decode::decode_editing(
        std::path::Path::new(&path),
        "render-example",
        std::path::Path::new(&out),
    )
    .expect("decode");
    println!("decode: {:?} ({}x{})", t.elapsed(), img.width, img.height);

    // Export path (split forced to 0 = full after).
    let (renderer, adapter) = laika_develop::Renderer::spawn(|_| {}).expect("renderer");
    println!("adapter: {adapter}");
    renderer.set_source(img.clone());

    for (name, params) in [
        ("default", laika_core::edit::defaults()),
        ("exposed", {
            let mut p = laika_core::edit::defaults();
            p[2] = 2.0;
            p
        }),
        ("warm", {
            let mut p = laika_core::edit::defaults();
            p[0] = 7000.0;
            p[2] = 1.0;
            p
        }),
    ] {
        let t = Instant::now();
        let frame = renderer
            .render_export(img.clone(), params, 0., Default::default())
            .expect("export");
        println!(
            "{name}: {:?} gpu={:.1}ms copy={:.1}ms",
            t.elapsed(),
            frame.gpu_ms,
            frame.copy_ms
        );
        save(&out, name, &frame);
    }

    // Preview path with before/after split, as the Develop canvas uses it.
    let (tx, rx) = std::sync::mpsc::channel();
    let (preview, _) = laika_develop::Renderer::spawn(move |frame| {
        tx.send(frame).ok();
    })
    .expect("preview renderer");
    preview.set_source(img.clone());
    let mut p = laika_core::edit::defaults();
    p[2] = 2.0;
    p[10] = 60.0;
    preview.submit(laika_develop::Job {
        params: p,
        locals: Default::default(),
        camera_profile: Default::default(),
        show_mask_overlay: false,
        split: 0.38,
        geom: Default::default(),
        preview_long_edge: laika_develop::PREVIEW_LONG_EDGE,
        photo_id: None,
        seq: 1,
    });
    let frame = rx
        .recv_timeout(std::time::Duration::from_secs(30))
        .expect("frame");
    println!(
        "split: gpu={:.1}ms copy={:.1}ms",
        frame.gpu_ms, frame.copy_ms
    );
    save(&out, "split", &frame);
}
