//! Reference command-line renderer for Laika catalogs and XMP sidecars.

use std::path::{Path, PathBuf};

use laika_core::reference::DevelopState;
use laika_export::{ExportFormat, ExportFormatOpts};
use rusqlite::{Connection, OpenFlags, OptionalExtension};

const USAGE: &str = "\
laika-render — render a Laika catalog edit or XMP sidecar without the GUI

USAGE
  laika-render --catalog CATALOG (--photo-id ID | --photo FILE) --output FILE [OPTIONS]
  laika-render --sidecar SIDECAR --photo FILE --output FILE [OPTIONS]
  laika-render --photo FILE --output FILE [OPTIONS]

The last form reads the normal sidecar beside FILE. Output must end in .jpg,
.jpeg, .tif, or .tiff.

OPTIONS
  --quality N       JPEG quality, 1..100 (default: 90)
  --help            Show this help
  --version         Show the version
";

#[derive(Debug, Default)]
struct Args {
    catalog: Option<PathBuf>,
    sidecar: Option<PathBuf>,
    photo: Option<PathBuf>,
    photo_id: Option<i64>,
    output: Option<PathBuf>,
    quality: u8,
}

fn value(iter: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    iter.next().ok_or_else(|| format!("{flag} needs a value"))
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Option<Args>, String> {
    let mut out = Args {
        quality: 90,
        ..Default::default()
    };
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                print!("{USAGE}");
                return Ok(None);
            }
            "--version" | "-V" => {
                println!("laika-render {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            "--catalog" => out.catalog = Some(PathBuf::from(value(&mut iter, "--catalog")?)),
            "--sidecar" => out.sidecar = Some(PathBuf::from(value(&mut iter, "--sidecar")?)),
            "--photo" => out.photo = Some(PathBuf::from(value(&mut iter, "--photo")?)),
            "--photo-id" => {
                out.photo_id = Some(
                    value(&mut iter, "--photo-id")?
                        .parse()
                        .map_err(|_| "--photo-id must be an integer".to_string())?,
                )
            }
            "--output" | "-o" => out.output = Some(PathBuf::from(value(&mut iter, "--output")?)),
            "--quality" => {
                out.quality = value(&mut iter, "--quality")?
                    .parse::<u8>()
                    .ok()
                    .filter(|q| (1..=100).contains(q))
                    .ok_or_else(|| "--quality must be from 1 to 100".to_string())?;
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }
    if out.catalog.is_some() && out.sidecar.is_some() {
        return Err("choose --catalog or --sidecar, not both".to_string());
    }
    if out.photo_id.is_some() && out.catalog.is_none() {
        return Err("--photo-id requires --catalog".to_string());
    }
    if out.catalog.is_some() && out.photo_id.is_none() && out.photo.is_none() {
        return Err("catalog mode needs --photo-id or --photo".to_string());
    }
    if out.catalog.is_none() && out.photo.is_none() {
        return Err("sidecar mode needs --photo".to_string());
    }
    if out.output.is_none() {
        return Err("--output is required".to_string());
    }
    Ok(Some(out))
}

fn schema_version(conn: &Connection) -> Result<u32, String> {
    let has_table: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='schema_version')",
            [],
            |r| r.get(0),
        )
        .map_err(|e| format!("read catalog schema: {e}"))?;
    if !has_table {
        return Ok(0);
    }
    conn.query_row("SELECT version FROM schema_version LIMIT 1", [], |r| {
        r.get(0)
    })
    .optional()
    .map(|v| v.unwrap_or(0))
    .map_err(|e| format!("read catalog schema: {e}"))
}

fn from_catalog(args: &Args) -> Result<(PathBuf, DevelopState), String> {
    let db = args.catalog.as_ref().expect("catalog checked");
    let conn = Connection::open_with_flags(
        db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("open catalog {}: {e}", db.display()))?;
    let version = schema_version(&conn)?;
    if version > laika_core::catalog::migrations::APP_SCHEMA_VERSION {
        return Err(format!(
            "catalog schema v{version} is newer than this renderer (v{})",
            laika_core::catalog::migrations::APP_SCHEMA_VERSION
        ));
    }

    let (id, path): (i64, String) = if let Some(id) = args.photo_id {
        conn.query_row("SELECT id, path FROM photos WHERE id = ?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()
        .map_err(|e| format!("read photo: {e}"))?
        .ok_or_else(|| format!("photo id {id} is not in the catalog"))?
    } else {
        let wanted = args.photo.as_ref().expect("selector checked");
        let text = wanted.to_string_lossy();
        conn.query_row(
            "SELECT id, path FROM photos WHERE path = ?1",
            [text.as_ref()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| format!("read photo: {e}"))?
        .ok_or_else(|| format!("{} is not in the catalog", wanted.display()))?
    };

    let row: Option<(Option<String>, Option<String>, Option<i64>)> = conn
        .query_row(
            "SELECT params_json, history_json, cursor FROM edits WHERE photo_id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(|e| format!("read edit for photo {id}: {e}"))?;
    let state = match row {
        Some((params, history, cursor)) => laika_core::reference::decode_catalog_edit(
            params.as_deref(),
            history.as_deref(),
            cursor.unwrap_or(0),
        )?,
        None => DevelopState::default(),
    };
    Ok((PathBuf::from(path), state))
}

fn from_sidecar(args: &Args) -> Result<(PathBuf, DevelopState), String> {
    let photo = args.photo.clone().expect("photo checked");
    let sidecar = args
        .sidecar
        .clone()
        .unwrap_or_else(|| PathBuf::from(laika_core::xmp::sidecar_path(&photo.to_string_lossy())));
    let bytes =
        std::fs::read(&sidecar).map_err(|e| format!("read sidecar {}: {e}", sidecar.display()))?;
    let parsed = laika_core::xmp::parse(&bytes)
        .ok_or_else(|| format!("{} is not a readable XMP sidecar", sidecar.display()))?;
    Ok((
        photo,
        DevelopState {
            params: parsed.params,
            geom: parsed.geom.unwrap_or_default(),
            ..Default::default()
        },
    ))
}

fn output_format(path: &Path) -> Result<ExportFormat, String> {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("jpg" | "jpeg") => Ok(ExportFormat::Jpeg),
        Some("tif" | "tiff") => Ok(ExportFormat::Tiff),
        _ => Err("output must end in .jpg, .jpeg, .tif, or .tiff".to_string()),
    }
}

fn render(args: Args) -> Result<PathBuf, String> {
    let output = args.output.clone().expect("output checked");
    let format = output_format(&output)?;
    let (photo, state) = if args.catalog.is_some() {
        from_catalog(&args)?
    } else {
        from_sidecar(&args)?
    };
    if !photo.is_file() {
        return Err(format!("original is not available: {}", photo.display()));
    }

    let source_path = photo.clone();
    let linear = laika_raw::on_big_stack(move || {
        if laika_raw::is_raw(&source_path) {
            laika_raw::decode::decode(&source_path)
        } else {
            laika_raw::decode::linear_from_raster(&source_path, None)
        }
    })??;
    let params = laika_core::reference::effective_params(&state);
    let (rect, warp) = laika_core::edit::constrain_geom(
        &state.geom,
        &params,
        linear.width as f32,
        linear.height as f32,
    );
    let geom = laika_develop::CropRender {
        rect,
        angle_rad: state.geom.angle.to_radians(),
        flip_h: state.geom.flip_h,
        flip_v: state.geom.flip_v,
        rotation: state.geom.rotation,
        frame_view: false,
        warp,
    };
    let (renderer, adapter) = laika_develop::Renderer::spawn(|_| {})?;
    eprintln!("renderer: {adapter}");
    let frame = renderer.render_export_with(
        linear,
        params,
        state.locals,
        state.camera_profile,
        0.,
        geom,
    )?;
    let rgba = image::RgbaImage::from_raw(frame.width, frame.height, frame.rgba)
        .ok_or_else(|| "renderer returned invalid pixels".to_string())?;
    let opts = ExportFormatOpts {
        quality: args.quality,
        ..Default::default()
    };
    let (bytes, _) = laika_export::encode_pixels(format, &opts, &rgba)?;
    if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create output directory {}: {e}", parent.display()))?;
    }
    laika_core::xmp::write_atomic(&output.to_string_lossy(), &bytes)?;
    eprintln!(
        "rendered {}x{} from {}",
        frame.width,
        frame.height,
        photo.display()
    );
    Ok(output)
}

fn main() {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(Some(args)) => args,
        Ok(None) => return,
        Err(e) => {
            eprintln!("error: {e}\n\n{USAGE}");
            std::process::exit(2);
        }
    };
    match render(args) {
        Ok(path) => println!("{}", path.display()),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_reject_ambiguous_sources() {
        let err = parse_args(
            [
                "--catalog",
                "a.db",
                "--sidecar",
                "a.xmp",
                "--photo",
                "a.nef",
                "--output",
                "a.jpg",
            ]
            .into_iter()
            .map(str::to_string),
        )
        .unwrap_err();
        assert!(err.contains("not both"));
    }

    #[test]
    fn format_is_inferred_strictly() {
        assert_eq!(
            output_format(Path::new("a.JPEG")).unwrap(),
            ExportFormat::Jpeg
        );
        assert_eq!(
            output_format(Path::new("a.tiff")).unwrap(),
            ExportFormat::Tiff
        );
        assert!(output_format(Path::new("a.png")).is_err());
    }

    #[test]
    fn reads_every_schema_version_without_writing() {
        let dir = std::env::temp_dir().join(format!("laika-render-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for version in 0..=laika_core::catalog::migrations::APP_SCHEMA_VERSION {
            let db = dir.join(format!("catalog-v{version}.db"));
            std::fs::remove_file(&db).ok();
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch(
                "CREATE TABLE photos(id INTEGER PRIMARY KEY, path TEXT NOT NULL);\
                 CREATE TABLE edits(photo_id INTEGER PRIMARY KEY, params_json TEXT, history_json TEXT, cursor INTEGER);\
                 INSERT INTO photos(id,path) VALUES(7,'/missing/test.nef');",
            )
            .unwrap();
            if version > 0 {
                conn.execute_batch("CREATE TABLE schema_version(version INTEGER);")
                    .unwrap();
                conn.execute("INSERT INTO schema_version VALUES(?1)", [version])
                    .unwrap();
            }
            let mut values = laika_core::edit::defaults();
            values[2] = 1.25;
            conn.execute(
                "INSERT INTO edits(photo_id,params_json,history_json,cursor) VALUES(7,?1,'[]',0)",
                rusqlite::params![serde_json::to_string(&values[..12]).unwrap()],
            )
            .unwrap();
            drop(conn);
            let before = std::fs::read(&db).unwrap();
            let args = Args {
                catalog: Some(db.clone()),
                photo_id: Some(7),
                ..Default::default()
            };
            let (path, edit) = from_catalog(&args).unwrap();
            assert_eq!(path, PathBuf::from("/missing/test.nef"));
            assert_eq!(edit.params[2], 1.25);
            assert_eq!(
                schema_version(&Connection::open(&db).unwrap()).unwrap(),
                version
            );
            assert_eq!(std::fs::read(&db).unwrap(), before, "schema v{version}");
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
