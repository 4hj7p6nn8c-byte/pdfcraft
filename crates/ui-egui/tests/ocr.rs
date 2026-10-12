//! Scan & OCR ▸ Recognize text in the real shell (egui_kittest): the dialog, the run and its
//! result (skipped without the OCR models: `cargo xtask models`).

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use pdfcraft_engine::{AddedText, Edit, Session, export};
use pdfcraft_ui_egui::PdfCraftApp;

/// A one-page PDF that is only a picture of a sentence.
fn scan() -> Vec<u8> {
    let mut s = Session::new();
    let text = s.create_from_text("t", "Recognize these scanned words").unwrap();
    let id = s.open("text.pdf", None, text, None).unwrap();
    let png = export::Exporter::new(s.get(id).unwrap()).png(0, 150.0).unwrap();
    s.create_from_images(&[("scan.png".into(), png)]).unwrap().to_vec()
}

#[test]
fn recognize_text_dialog_adds_searchable_text() {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_eframe(|_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("scan.pdf", None, scan()).unwrap();
        app.run_inline = true;
        app
    });
    h.run_steps(4);
    assert!(h.state_mut().execute("ocr.recognize"));
    h.run_steps(2);
    h.get_by_label("Document language");
    h.get_by_label("Output");
    if !pdfcraft_engine::ocr::available() {
        eprintln!("skipped: OCR models not installed");
        return;
    }
    h.get_by_label("Recognize text").click();
    h.run_steps(3);
    assert!(h.state().ocr_run.is_none(), "finished");
    let toast = h.state().toast.clone().unwrap().0;
    assert!(toast.starts_with("Recognized ") && toast.contains("on 1 page"), "{toast}");
    let app = h.state();
    let id = app.active_ids().unwrap().1;
    assert_eq!(app.session.get(id).unwrap().can_undo(), Some("Recognize text"));
}

#[test]
fn recognize_text_in_multiple_files_writes_searchable_copies() {
    if !pdfcraft_engine::ocr::available() {
        eprintln!("skipped: OCR models not installed");
        return;
    }
    let dir = std::env::temp_dir().join(format!("pdfcraft-ocr-ui-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut app = PdfCraftApp::new();
    app.set_option("language", "en").unwrap();
    app.run_inline = true;
    app.export_dir_override = Some(dir.to_string_lossy().into_owned());
    app.use_files(pdfcraft_ui_egui::FilePurpose::Ocr, vec![("one.pdf".into(), scan()), ("two.pdf".into(), scan())]);
    assert!(app.ocr_batch.is_none(), "finished");
    let toast = app.toast.clone().unwrap().0;
    assert!(toast.starts_with("Recognized ") && toast.ends_with("in 2 files"), "{toast}");
    for name in ["one.pdf", "two.pdf"] {
        let bytes = std::fs::read(dir.join(name)).unwrap();
        assert!(bytes.len() > scan().len(), "an incremental update was appended");
    }
}

/// #1028: "Also recognize pages that already have text" reads a picture on a page that has its own
/// text, without adding that text a second time.
#[test]
fn recognize_text_can_read_pictures_on_pages_with_text() {
    let hybrid = {
        let mut s = Session::new();
        let id = s.open("scan.pdf", None, std::sync::Arc::new(scan()), None).unwrap();
        let (w, h) = {
            let p = &s.get(id).unwrap().info.pages[0];
            (p.width as f64, p.height as f64)
        };
        let note =
            AddedText { rect: [w * 0.1, h * 0.75, w * 0.9, h * 0.85], text: "Invoice reference 4711".into(), size: 18.0, ..Default::default() };
        s.apply(id, Edit::AddText { page: 0, text: note }).unwrap();
        s.save_bytes(id).unwrap().to_vec()
    };
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_eframe(|_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("hybrid.pdf", None, hybrid).unwrap();
        app.run_inline = true;
        app
    });
    h.run_steps(4);
    assert!(h.state_mut().execute("ocr.recognize"));
    h.run_steps(2);
    assert!(h.state().ocr_draft.settings().skip_text_pages, "pages with text are skipped by default");
    h.get_by_label("Also recognize pages that already have text").click();
    h.run_steps(2);
    assert!(!h.state().ocr_draft.settings().skip_text_pages);
    if !pdfcraft_engine::ocr::available() {
        eprintln!("skipped: OCR models not installed");
        return;
    }
    h.get_by_label("Recognize text").click();
    h.run_steps(3);
    assert!(h.state().ocr_run.is_none(), "finished");
    let toast = h.state().toast.clone().unwrap().0;
    assert!(toast.starts_with("Recognized ") && toast.ends_with("on 1 page"), "{toast}");
    let app = h.state();
    let id = app.active_ids().unwrap().1;
    let doc = app.session.get(id).unwrap();
    assert_eq!(doc.can_undo(), Some("Recognize text"));
    let mut r = pdfcraft_render::PageRenderer::new(doc.bytes.clone(), Default::default());
    let text = r.render(pdfcraft_render::RenderRequest { page: 0, kind: pdfcraft_render::RequestKind::Text, scale: 1.0, ..Default::default() });
    let text = text.text.unwrap().plain_text().to_lowercase();
    assert_eq!((text.matches("invoice").count(), text.matches("scanned").count()), (1, 1), "{text}");
}

fn protected_scan() -> Vec<u8> {
    let mut doc = pdfcraft_cos::Document::open(std::sync::Arc::new(scan())).unwrap();
    doc.set_encryption(&pdfcraft_cos::NewEncryption {
        algorithm: pdfcraft_cos::Algorithm::Rc4_128,
        user_password: "reader",
        owner_password: "owner",
        permissions: (1 << 2) | (1 << 9) | (1 << 11),
        encrypt_metadata: true,
        seed: [7; 32],
    })
    .unwrap();
    pdfcraft_cos::write_full(&doc, &Default::default()).unwrap()
}

#[test]
fn ocr_copy_dialog_suggests_copy_for_restricted_source_and_requires_authorization() {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1000.0)).build_eframe(|_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("protected.pdf", None, protected_scan()).unwrap();
        app.submit_password(Some("reader".into()));
        app
    });
    h.run_steps(3);
    assert!(h.state_mut().execute("ocr.recognize"), "command is reachable despite modify restriction");
    h.run_steps(2);
    assert_eq!(h.state().ocr_draft.mode, pdfcraft_ui_egui::OcrMode::Copy);
    h.get_by_label("Save as New Searchable PDF");
    h.get_by_label("I am authorized to create this unencrypted searchable derivative");
    if let Ok(path) = std::env::var("PDFCRAFT_OCR_COPY_SHOT") {
        h.render().unwrap().save(path).unwrap();
    }
    h.state_mut().start_ocr();
    assert!(h.state().ocr_run.is_none());
    assert!(h.state().toast.as_ref().unwrap().0.contains("unencrypted"));
}

#[test]
fn ocr_copy_ui_saves_opens_separate_tab_and_preserves_original() {
    if !pdfcraft_engine::ocr::available() {
        eprintln!("skipped: OCR models not installed");
        return;
    }
    let dir = std::env::temp_dir().join(format!("pdfcraft-ocr-copy-ui-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("protected.pdf");
    let target = dir.join("protected_OCR.pdf");
    // A previous run's completed copy is an intentional collision until removed by this test.
    if target.exists() {
        std::fs::remove_file(&target).unwrap();
    }
    let original = protected_scan();
    std::fs::write(&source, &original).unwrap();
    let mut app = PdfCraftApp::new();
    app.set_option("language", "en").unwrap();
    app.open_bytes("protected.pdf", Some(source.to_string_lossy().into_owned()), original.clone()).unwrap();
    app.submit_password(Some("reader".into()));
    let id = app.active_ids().unwrap().1;
    let before = app.session.get(id).unwrap().bytes.clone();
    app.run_inline = true;
    app.save_override = Some(target.to_string_lossy().into_owned());
    assert!(app.execute("ocr.recognize"));
    app.ocr_draft.authorize_derivative = true;
    app.start_ocr();
    assert!(target.exists(), "{}", app.toast.as_ref().unwrap().0);
    assert_eq!(app.views.len(), 2);
    assert_ne!(app.active_ids().unwrap().1, id);
    let doc = app.session.get(id).unwrap();
    assert_eq!(doc.bytes, before);
    assert!(!doc.dirty && doc.can_undo().is_none());
    assert_eq!(std::fs::read(source).unwrap(), original);
    let copy = std::fs::read(&target).unwrap();
    app.active = Some(0);
    app.start_ocr();
    assert!(app.toast.as_ref().unwrap().0.contains("already exists"));
    assert_eq!(std::fs::read(&target).unwrap(), copy);
}
