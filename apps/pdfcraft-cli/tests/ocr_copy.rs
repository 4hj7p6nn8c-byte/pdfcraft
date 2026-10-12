//! Exercise the shared searchable-copy tool through the actual CLI script transport.
use serde_json::json;
use std::process::Command;

#[test]
fn cli_ocr_copy_is_searchable_and_preserves_source_under_root() {
    if !pdfcraft_engine::ocr::available() {
        eprintln!("skipped: OCR models not installed");
        return;
    }
    let root = std::env::temp_dir().join(format!("pdfcraft-cli-ocr-copy-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let output = root.join("source_OCR.pdf");
    if output.exists() {
        std::fs::remove_file(&output).unwrap();
    }
    let mut s = pdfcraft_engine::Session::new();
    let text = s.create_from_text("t", "Searchable copies leave the original untouched").unwrap();
    let id = s.open("text.pdf", None, text, None).unwrap();
    let png = pdfcraft_engine::export::Exporter::new(s.get(id).unwrap()).png(0, 150.0).unwrap();
    let bytes = s.create_from_images(&[("scan.png".into(), png)]).unwrap();
    std::fs::write(root.join("source.pdf"), &*bytes).unwrap();
    let script = root.join("steps.json");
    std::fs::write(
        &script,
        json!([
            { "tool": "doc_open", "args": { "path": "source.pdf" } },
            { "tool": "ocr_create_searchable_copy", "args": { "doc": 1, "path": "source_OCR.pdf", "open": true } },
            { "tool": "text_find", "args": { "doc": 2, "query": "untouched" } }
        ])
        .to_string(),
    )
    .unwrap();
    // Embedded builds must also work from a relocated executable with no neighbouring models.
    let exe = if cfg!(feature = "embedded-ocr") {
        let app_dir = root.join("app");
        std::fs::create_dir_all(&app_dir).unwrap();
        let exe = app_dir.join(format!("pdfcraft-cli{}", std::env::consts::EXE_SUFFIX));
        std::fs::copy(env!("CARGO_BIN_EXE_pdfcraft-cli"), &exe).unwrap();
        exe
    } else {
        env!("CARGO_BIN_EXE_pdfcraft-cli").into()
    };
    let run = || {
        let mut cmd = Command::new(&exe);
        cmd.args(["run", "--script"]).arg(&script).arg("--root").arg(&root).current_dir(&root);
        if cfg!(feature = "embedded-ocr") {
            cmd.env("PDFCRAFT_MODELS", root.join("missing-models"));
        }
        cmd.output().unwrap()
    };
    let r = run();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let stdout = String::from_utf8_lossy(&r.stdout);
    assert!(stdout.contains("faithful") && stdout.contains("\"page\""), "{stdout}");
    assert!(stdout.contains("\"count\": 1"), "the exported copy must have one actual search hit: {stdout}");
    assert!(output.exists());
    assert_eq!(std::fs::read(root.join("source.pdf")).unwrap(), *bytes);
    let r = run();
    assert!(!r.status.success());
    assert!(String::from_utf8_lossy(&r.stderr).contains("already exists"));
    if cfg!(feature = "embedded-ocr") {
        assert!(!root.join("app/models").exists());
        assert!(!root.join("missing-models").exists());
    }
}
