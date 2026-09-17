//! Phase 4 end-to-end: catalog queue → MinIO upload+verify → synced.
//! Gated on LAIKA_SYNC_TEST=1 + LAIKA_S3_* env (local MinIO bucket must exist).

use std::path::{Path, PathBuf};

fn settings_from_env() -> laika_core::sync::SyncSettings {
    let mut s = laika_core::sync::SyncSettings {
        endpoint: std::env::var("LAIKA_S3_ENDPOINT").unwrap(),
        bucket: std::env::var("LAIKA_S3_BUCKET").unwrap(),
        region: std::env::var("LAIKA_S3_REGION").unwrap_or("us-east-1".into()),
        access_key: std::env::var("LAIKA_S3_KEY").unwrap(),
        target: laika_core::sync::BackupTarget::S3,
        ..Default::default()
    };
    s.apply_env();
    s
}

fn jpeg(path: &Path, w: u32, h: u32) {
    let img = image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(w, h, |x, y| {
        image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
    }));
    img.save(path).unwrap();
}

#[test]
fn phase4_queue_to_minio() {
    if std::env::var("LAIKA_SYNC_TEST").as_deref() != Ok("1") {
        return;
    }
    let settings = settings_from_env();
    let secret = std::env::var("LAIKA_S3_SECRET").unwrap();
    assert!(settings.configured());

    let dir: PathBuf =
        std::env::temp_dir().join(format!("laika-p4-{}-{}", std::process::id(), rand_suffix()));
    std::fs::create_dir_all(&dir).unwrap();
    let shoot = dir.join("shoot");
    std::fs::create_dir_all(&shoot).unwrap();
    jpeg(&shoot.join("a.jpg"), 64, 48);
    jpeg(&shoot.join("b.jpg"), 64, 48);

    let cat = laika_core::catalog::Catalog::open(&dir.join("cat.db"), "Weddings", &dir).unwrap();
    let cache = dir.join("cache");
    let mut ids = Vec::new();
    for f in ["a.jpg", "b.jpg"] {
        let id = cat.import_file(&shoot.join(f), &cache).unwrap().unwrap();
        ids.push(id);
    }
    assert_eq!(cat.photo_count(), 2);

    // Enqueue originals, drain with concurrency-1 worker semantics.
    let n = cat.enqueue_unsynced();
    assert_eq!(n, 2);
    let catalog_name = cat.catalog_name();
    let mut uploaded = 0;
    while let Some(job) = cat.claim_job() {
        let (local, hash, captured) = cat.job_inputs(job.photo_id, &job.kind).expect("inputs");
        assert_eq!(job.kind, "original");
        let remote = laika_core::sync::remote_key(&catalog_name, &captured, &local, false);
        assert!(remote.starts_with("weddings/"), "key layout: {remote}");
        let bytes =
            laika_core::sync::upload_blocking(&settings, &secret, &local, &remote, &hash).unwrap();
        assert!(bytes > 0);
        cat.complete_job(&job, Some(&remote));
        uploaded += 1;
    }
    assert_eq!(uploaded, 2);
    assert_eq!(cat.queue_depth(), (0, 0));
    assert!(
        cat.all_photos()
            .iter()
            .all(|p| matches!(p.sync, laika_core::photo::SyncState::Synced))
    );
    assert!(cat.all_photos().iter().all(|p| !p.remote_key.is_empty()));

    // Sidecar change re-enqueues the sidecar only.
    let first = &cat.all_photos()[0];
    laika_core::xmp::write(
        &first.path,
        &[0.0; laika_core::edit::PARAM_COUNT],
        3,
        &[],
        None,
        &laika_core::xmp::Authorship::default(),
        &Default::default(),
    )
    .unwrap();
    cat.enqueue(first.id, "sidecar");
    let job = cat.claim_job().expect("sidecar job");
    assert_eq!(job.kind, "sidecar");
    let (local, hash, captured) = cat
        .job_inputs(job.photo_id, &job.kind)
        .expect("sidecar inputs");
    assert!(local.extension().and_then(|e| e.to_str()) == Some("xmp"));
    let orig_path = first.path.clone();
    let remote =
        laika_core::sync::remote_key(&catalog_name, &captured, Path::new(&orig_path), true);
    assert!(remote.ends_with(".xmp"), "{remote}");
    laika_core::sync::upload_blocking(&settings, &secret, &local, &remote, &hash).unwrap();
    cat.complete_job(&job, Some(&remote));

    // Failure + retry path.
    cat.enqueue(ids[1], "original");
    let job = cat.claim_job().expect("job to fail");
    cat.fail_job(&job, "simulated outage");
    assert_eq!(cat.queue_depth(), (0, 1));
    assert!(
        cat.all_photos()
            .iter()
            .any(|p| matches!(p.sync, laika_core::photo::SyncState::Failed))
    );
    assert_eq!(cat.retry_failed(None), 1);
    let job = cat.claim_job().expect("retried job");
    let (local, hash, captured) = cat.job_inputs(job.photo_id, &job.kind).unwrap();
    let remote = laika_core::sync::remote_key(&catalog_name, &captured, &local, false);
    laika_core::sync::upload_blocking(&settings, &secret, &local, &remote, &hash).unwrap();
    cat.complete_job(&job, Some(&remote));
    assert_eq!(cat.queue_depth(), (0, 0));

    std::fs::remove_dir_all(&dir).ok();
}

fn rand_suffix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0)
}
