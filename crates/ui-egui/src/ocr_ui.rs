//! Scan & OCR ▸ Recognize text: the Recognize Text dialog (pages, language, output, resolution)
//! and the background run with its progress. Recognition happens on a worker thread; the result
//! is applied as one undoable edit when it is done.

use std::sync::{Arc, Mutex};

use egui::{Align, Layout};
use pdfcraft_engine::DocId;
use pdfcraft_engine::ocr::{CopyJob, CopyKind, CopyResult, LANGUAGES, OcrPage, OcrSettings};

use crate::theme::{self, Tokens};
use crate::{PdfCraftApp, widgets};

/// Which pages to read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OcrPages {
    All,
    Current,
    Range,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OcrMode {
    Modify,
    Copy,
}

/// The dialog's choices (kept between runs, as in Acrobat).
#[derive(Clone, Debug, PartialEq)]
pub struct OcrDraft {
    pub mode: OcrMode,
    pub open_copy: bool,
    pub overwrite: bool,
    pub authorize_derivative: bool,
    pub pages: OcrPages,
    pub from: usize,
    pub to: usize,
    pub language: String,
    /// "Downsample to" resolution.
    pub dpi: u32,
    /// Also recognize pages that already have text (pictures on them; their text isn't added again).
    pub text_pages: bool,
}

impl Default for OcrDraft {
    fn default() -> Self {
        OcrDraft {
            mode: OcrMode::Modify,
            text_pages: false,
            open_copy: true,
            overwrite: false,
            authorize_derivative: false,
            pages: OcrPages::All,
            from: 1,
            to: 1,
            language: "en".into(),
            dpi: 300,
        }
    }
}

impl OcrDraft {
    /// The recognition settings these choices make.
    pub fn settings(&self) -> OcrSettings {
        OcrSettings { dpi: self.dpi as f32, language: self.language.clone(), skip_text_pages: !self.text_pages }
    }
}

/// Progress of a run: pages done, total, the result once finished, and a cancel request.
#[derive(Default)]
pub struct OcrProgress {
    pub done: usize,
    pub total: usize,
    pub result: Option<Result<OcrOutput, String>>,
    pub cancel: bool,
}

pub enum OcrOutput {
    Modify(Vec<OcrPage>),
    Copy { result: CopyResult, path: String, open: bool },
}

/// Recognize text in multiple files: files done, total, and the summary once finished.
#[derive(Default)]
pub struct BatchProgress {
    pub done: usize,
    pub total: usize,
    pub message: Option<String>,
}

pub struct OcrRun {
    pub doc: DocId,
    pub progress: Arc<Mutex<OcrProgress>>,
}

pub(crate) fn body(ui: &mut egui::Ui, app: &mut PdfCraftApp, t: &Tokens) -> (bool, bool) {
    let pages = app.active_ids().and_then(|(_, id)| app.session.get(id)).map_or(1, |d| d.info.pages.len().max(1));
    let available = pdfcraft_engine::ocr::available();
    let id = app.active_ids().map(|(_, id)| id);
    let modify = id.and_then(|id| app.session.get(id)).is_some_and(|d| d.allows_modification());
    let policy = id.ok_or_else(|| "Open a PDF first".to_string()).and_then(|id| app.session.ocr_copy_policy(id));
    let raster = policy.as_ref().is_ok_and(|p| p.kind == CopyKind::Raster);
    let suggested = id.and_then(|id| app.session.get(id)).map(|d| pdfcraft_engine::ocr::suggested_copy_name(&d.name)).unwrap_or_default();
    let d = &mut app.ocr_draft;
    d.to = d.to.clamp(1, pages);
    d.from = d.from.clamp(1, d.to);
    ui.label(egui::RichText::new(tl!("Recognize Text")).font(theme::semibold(18.0)));
    ui.add_space(8.0);
    ui.add_enabled_ui(modify, |ui| widgets::radio_value(ui, &mut d.mode, OcrMode::Modify, "Modify Current PDF"));
    widgets::radio_value(ui, &mut d.mode, OcrMode::Copy, tl!("Save as New Searchable PDF"));
    if !modify {
        ui.label("Editing is restricted. A separate searchable copy is suggested.");
    }
    if d.mode == OcrMode::Copy {
        ui.label(format!("Suggested filename: {suggested}"));
        ui.add(widgets::checkbox(&mut d.open_copy, "Open the copy in a new tab"));
        ui.add(widgets::checkbox(&mut d.overwrite, "Allow replacing an existing destination (never the source)"));
        match &policy {
            Ok(p) => {
                for warning in &p.warnings {
                    ui.label(egui::RichText::new(warning).small().color(t.text_muted));
                }
                if raster {
                    ui.add(widgets::checkbox(&mut d.authorize_derivative, "I am authorized to create this unencrypted searchable derivative"));
                    if p.max_dpi < 600.0 {
                        ui.label(format!("Printing is limited to {} dpi.", p.max_dpi));
                    }
                }
            }
            Err(e) => {
                ui.label(egui::RichText::new(e).color(t.text_muted));
            }
        }
    }
    ui.add_space(8.0);
    let group = |ui: &mut egui::Ui, title: &str, body: &mut dyn FnMut(&mut egui::Ui)| {
        ui.label(egui::RichText::new(tl!(title)).font(theme::semibold(13.0)));
        egui::Frame::new().fill(t.hover).corner_radius(egui::CornerRadius::same(6)).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            body(ui);
        });
        ui.add_space(8.0);
    };
    group(ui, "Pages", &mut |ui| {
        widgets::radio_value(ui, &mut d.pages, OcrPages::All, tl!("All pages"));
        widgets::radio_value(ui, &mut d.pages, OcrPages::Current, tl!("Current page"));
        ui.horizontal(|ui| {
            widgets::radio_value(ui, &mut d.pages, OcrPages::Range, tl!("From"));
            let on = d.pages == OcrPages::Range;
            ui.add_enabled(on, egui::DragValue::new(&mut d.from).range(1..=pages));
            ui.label(tl!("to"));
            ui.add_enabled(on, egui::DragValue::new(&mut d.to).range(1..=pages));
        });
    });
    ui.label(
        egui::RichText::new("The installed OCR models support English/Latin text. Chinese and other scripts are not supported.")
            .small()
            .color(t.text_muted),
    );
    group(ui, "Settings", &mut |ui| {
        egui::Grid::new("ocr-settings").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label(tl!("Document language"));
            let name = LANGUAGES.iter().find(|l| l.0 == d.language).map_or("English", |l| l.1);
            egui::ComboBox::from_id_salt("ocr-language").selected_text(tl!(name)).width(220.0).show_ui(ui, |ui| {
                for (code, name) in LANGUAGES {
                    ui.selectable_value(&mut d.language, (*code).to_string(), tl!(*name));
                }
            });
            ui.end_row();
            ui.label(tl!("Output"));
            let output_label = if d.mode == OcrMode::Copy && raster { "Searchable raster image" } else { tl!("Searchable Image (Exact)") };
            egui::ComboBox::from_id_salt("ocr-output").selected_text(output_label).width(220.0).show_ui(ui, |ui| {
                let _ = ui.selectable_label(true, output_label).on_hover_text(if raster && d.mode == OcrMode::Copy {
                    "Renders visible pages into a separate PDF and adds invisible text. Original PDF structure and security are lost."
                } else {
                    tl!("Adds invisible text over each word; the page image is not changed")
                });
            });
            ui.end_row();
            ui.label(tl!("Downsample to"));
            egui::ComboBox::from_id_salt("ocr-dpi").selected_text(format!("{} dpi", d.dpi)).width(220.0).show_ui(ui, |ui| {
                for v in [600, 300, 150, 72] {
                    ui.selectable_value(&mut d.dpi, v, format!("{v} dpi"));
                }
            });
            ui.end_row();
        });
        ui.add_space(4.0);
        ui.add(crate::widgets::checkbox(&mut d.text_pages, tl!("Also recognize pages that already have text")))
            .on_hover_text(tl!("Pictures on those pages are recognized too; words already in the page's text are not added again"));
    });
    if !available {
        ui.label(
            egui::RichText::new(tl!("Text recognition isn't installed: its model files are missing. Reinstall PdfCraft, or set PDFCRAFT_MODELS to the folder that holds them."))
                .small()
                .color(t.text_muted),
        );
        ui.add_space(6.0);
    }
    ui.add_space(6.0);
    let (mut go, mut cancel) = (false, false);
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let permitted = match d.mode {
                OcrMode::Modify => modify,
                OcrMode::Copy => policy.as_ref().is_ok_and(|p| d.dpi as f32 <= p.max_dpi) && (!raster || d.authorize_derivative),
            };
            if ui.add_enabled_ui(available && permitted, |ui| widgets::pill_button(ui, tl!("Recognize text"), true)).inner.clicked() {
                go = true;
            }
            if widgets::pill_button(ui, tl!("Cancel"), false).clicked() {
                cancel = true;
            }
        })
    });
    (go, cancel)
}

impl PdfCraftApp {
    /// Recognize text on the pages chosen in the dialog, in the background.
    pub fn start_ocr(&mut self) {
        let Some((vi, id)) = self.active_ids() else { return };
        if self.ocr_run.is_some() || self.optimize_run.is_some() {
            self.notify_tr("Text recognition is already running");
            return;
        }
        let d = &self.ocr_draft;
        let pages: Vec<usize> = match d.pages {
            OcrPages::All => Vec::new(),
            OcrPages::Current => vec![self.views[vi].current],
            OcrPages::Range => (d.from.saturating_sub(1)..d.to).collect(),
        };
        let settings = d.settings();
        if d.mode == OcrMode::Copy {
            let (overwrite, open) = (d.overwrite, d.open_copy);
            let job = match self.session.ocr_copy_job(id, &pages, settings, d.authorize_derivative) {
                Ok(j) => j,
                Err(e) => {
                    self.notify_error(e);
                    return;
                }
            };
            #[cfg(not(target_arch = "wasm32"))]
            {
                if let Some(path) = self.save_override.clone() {
                    self.start_copy_ocr(id, job, path, overwrite, open);
                } else {
                    let Some(doc) = self.session.get(id) else { return };
                    let mut dialog =
                        rfd::AsyncFileDialog::new().add_filter("PDF", &["pdf"]).set_file_name(pdfcraft_engine::ocr::suggested_copy_name(&doc.name));
                    if let Some(parent) = doc.path.as_deref().and_then(|p| std::path::Path::new(p).parent()) {
                        dialog = dialog.set_directory(parent);
                    }
                    self.ask_one(crate::pickers::Ask::Save(dialog), None, move |app, path| {
                        app.start_copy_ocr(id, job, path.to_string_lossy().into_owned(), overwrite, open)
                    });
                }
            }
            #[cfg(target_arch = "wasm32")]
            {
                let name =
                    self.session.get(id).map(|d| pdfcraft_engine::ocr::suggested_copy_name(&d.name)).unwrap_or_else(|| "document_OCR.pdf".into());
                self.start_copy_ocr(id, job, name, overwrite, open);
            }
            return;
        }
        if self.session.get(id).is_none_or(|d| !d.allows_modification()) {
            self.notify_error("In-place OCR requires modification permission. Use Save as New Searchable PDF or reopen with the owner password.");
            return;
        }
        let Some(job) = self.session.ocr_job(id, &pages, settings) else { return };
        self.launch_ocr(id, job.pages.len(), move |p| {
            let ocr = pdfcraft_engine::ocr::engine()?;
            Ok(OcrOutput::Modify(job.run(&ocr, |done, total| update_progress(p, done, total))))
        });
    }

    fn start_copy_ocr(&mut self, id: DocId, job: CopyJob, path: String, overwrite: bool, open: bool) {
        if self.ocr_run.is_some() || self.optimize_run.is_some() {
            self.notify_error("Another document job is already running");
            return;
        }
        let source = job.source_path().map(str::to_owned);
        #[cfg(not(target_arch = "wasm32"))]
        if let Err(e) =
            pdfcraft_platform::staging::check_copy_destination(std::path::Path::new(&path), source.as_deref().map(std::path::Path::new), overwrite)
        {
            self.notify_error(e.to_string());
            return;
        }
        self.launch_ocr(id, job.page_count(), move |p| {
            let ocr = pdfcraft_engine::ocr::engine()?;
            let result = job.run(&ocr, |done, total| update_progress(p, done, total))?;
            #[cfg(not(target_arch = "wasm32"))]
            pdfcraft_platform::staging::write_copy_atomic(
                std::path::Path::new(&path),
                source.as_deref().map(std::path::Path::new),
                &result.file.bytes,
                overwrite,
                || p.lock().is_ok_and(|s| !s.cancel),
            )
            .map_err(|e| e.to_string())?;
            #[cfg(target_arch = "wasm32")]
            {
                let _ = (source, overwrite);
                crate::editing::download(&path, &result.file.bytes)?;
            }
            Ok(OcrOutput::Copy { result, path, open })
        });
    }

    fn launch_ocr(&mut self, id: DocId, total: usize, work: impl FnOnce(&Arc<Mutex<OcrProgress>>) -> Result<OcrOutput, String> + Send + 'static) {
        let progress = Arc::new(Mutex::new(OcrProgress { total, ..Default::default() }));
        let p = progress.clone();
        let work = move || {
            let result = pdfcraft_engine::guard(|| work(&p)).and_then(|r| r);
            if let Ok(mut s) = p.lock() {
                s.result = Some(result);
            }
        };
        self.ocr_run = Some(OcrRun { doc: id, progress });
        #[cfg(not(target_arch = "wasm32"))]
        if self.run_inline {
            work();
        } else if let Err(e) = std::thread::Builder::new().name("pdfcraft-ocr".into()).spawn(work) {
            self.ocr_run = None;
            self.notify_error(format!("Could not start OCR worker: {e}"));
        }
        #[cfg(target_arch = "wasm32")]
        work();
        self.poll_ocr();
    }

    /// Recognize text in each file, writing the searchable copies into a folder the user picks
    /// (the export folder override in tests), under the same names.
    pub fn ocr_files(&mut self, files: Vec<(String, Vec<u8>)>) {
        if self.ocr_batch.is_some() {
            self.notify_tr("Text recognition is already running");
            return;
        }
        // The settings showing now, not when the folder arrives.
        let settings = self.ocr_draft.settings();
        #[cfg(not(target_arch = "wasm32"))]
        match self.export_dir_override.clone() {
            Some(d) => self.ocr_files_into(files, settings, d.into()),
            None => {
                let dialog = rfd::AsyncFileDialog::new().set_title(tl!("Choose a folder for the searchable files").to_string());
                self.ask_one(crate::pickers::Ask::Folder(dialog), None, move |app, dir| app.ocr_files_into(files, settings, dir));
            }
        }
        #[cfg(target_arch = "wasm32")]
        self.ocr_files_into(files, settings);
    }

    /// [`Self::ocr_files`] once the folder for the searchable files is known.
    fn ocr_files_into(&mut self, files: Vec<(String, Vec<u8>)>, settings: OcrSettings, #[cfg(not(target_arch = "wasm32"))] dir: std::path::PathBuf) {
        // Another batch may have started while the folder picker was open.
        if self.ocr_batch.is_some() {
            self.notify_tr("Text recognition is already running");
            return;
        }
        let progress = Arc::new(Mutex::new(BatchProgress { total: files.len(), ..Default::default() }));
        let p = progress.clone();
        // The summary is written on the worker thread: draw it in the UI's language.
        let lang = crate::i18n::current();
        let work = move || {
            crate::i18n::set_current(lang);
            let (mut ok, mut words, mut failed) = (0, 0, Vec::new());
            match pdfcraft_engine::ocr::engine() {
                Err(e) => failed.push(e),
                Ok(ocr) => {
                    for (i, (name, bytes)) in files.into_iter().enumerate() {
                        if let Ok(mut s) = p.lock() {
                            s.done = i;
                        }
                        let r = pdfcraft_engine::ocr::recognize_file(&name, Arc::new(bytes), None, settings.clone(), &ocr, |_, _| true);
                        let saved = r.and_then(|r| {
                            #[cfg(not(target_arch = "wasm32"))]
                            pdfcraft_platform::staging::write_copy_atomic(&dir.join(&name), None, &r.bytes, false, || true)
                                .map_err(|e| e.to_string())?;
                            #[cfg(target_arch = "wasm32")]
                            crate::editing::download(&name, &r.bytes)?;
                            Ok(r.words())
                        });
                        match saved {
                            Ok(n) => {
                                ok += 1;
                                words += n;
                            }
                            Err(e) => failed.push(format!("{name}: {}", e)),
                        }
                    }
                }
            }
            let done = if ok == 1 {
                crate::i18n::fmt(tl!("Recognized {w} words in 1 file"), &[("w", &words.to_string())])
            } else {
                crate::i18n::fmt(tl!("Recognized {w} words in {n} files"), &[("w", &words.to_string()), ("n", &ok.to_string())])
            };
            let mut msg = done;
            if !failed.is_empty() {
                msg.push_str(&crate::i18n::fmt(tl!("; failed: {list}"), &[("list", &failed.join("; "))]));
            }
            if let Ok(mut s) = p.lock() {
                s.done = s.total;
                s.message = Some(msg);
            }
        };
        #[cfg(not(target_arch = "wasm32"))]
        if self.run_inline {
            work();
        } else {
            std::thread::Builder::new().name("pdfcraft-ocr-files".into()).spawn(work).ok();
        }
        #[cfg(target_arch = "wasm32")]
        work();
        self.ocr_batch = Some(progress);
        self.poll_ocr();
    }

    /// Stop a running recognition (the pages read so far are kept).
    pub fn cancel_ocr(&mut self) {
        if let Some(r) = &self.ocr_run
            && let Ok(mut s) = r.progress.lock()
        {
            s.cancel = true;
        }
    }

    /// Show progress; apply the result once the worker is done.
    pub(crate) fn poll_ocr(&mut self) {
        if let Some(b) = self.ocr_batch.clone() {
            let msg = b.lock().ok().map(|mut s| s.message.take().ok_or((s.done, s.total)));
            match msg {
                Some(Ok(m)) => {
                    self.ocr_batch = None;
                    self.notify(m);
                }
                Some(Err((done, total))) => {
                    let m = crate::i18n::fmt(
                        tl!("Recognizing text… file {d} of {t}"),
                        &[("d", &(done + 1).min(total.max(1)).to_string()), ("t", &total.max(1).to_string())],
                    );
                    if self.toast.as_ref().is_none_or(|t| t.0 != m) {
                        self.notify(m);
                    }
                    if let Some(ctx) = &self.ctx {
                        ctx.request_repaint_after(std::time::Duration::from_millis(200));
                    }
                }
                None => self.ocr_batch = None,
            }
        }
        let Some(run) = self.ocr_run.as_ref() else { return };
        let (doc, progress) = (run.doc, run.progress.clone());
        let Ok(mut s) = progress.lock() else { return };
        let Some(result) = s.result.take() else {
            let msg = crate::i18n::fmt(
                tl!("Recognizing text… page {d} of {t}"),
                &[("d", &(s.done + 1).min(s.total.max(1)).to_string()), ("t", &s.total.max(1).to_string())],
            );
            self.progress_notice = Some(widgets::ProgressNotice {
                label: if s.cancel { "Cancelling…".into() } else { msg.clone() },
                fraction: s.done as f32 / s.total.max(1) as f32,
                cancellable: !s.cancel,
            });
            drop(s);
            if self.toast.as_ref().is_none_or(|t| t.0 != msg) {
                self.notify(msg);
            }
            if let Some(ctx) = &self.ctx {
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
            return;
        };
        drop(s);
        self.ocr_run = None;
        self.progress_notice = None;
        let found = match result {
            Ok(OcrOutput::Modify(f)) => f,
            Ok(OcrOutput::Copy { result, path, open }) => {
                let read = result.file.pages.iter().filter(|p| p.skipped.is_none()).count();
                let details: Vec<String> =
                    result.file.pages.iter().filter_map(|p| p.skipped.as_ref().map(|s| format!("page {}: {s}", p.page + 1))).collect();
                let mut message = format!("Saved {}: recognized {} words on {} pages", path, result.file.words(), read);
                if !details.is_empty() {
                    message.push_str(&format!("; skipped {}", details.join("; ")));
                }
                if !result.warnings.is_empty() {
                    message.push_str(&format!(". {}", result.warnings.join(" ")));
                }
                if open {
                    let name = std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "OCR.pdf".into());
                    if let Err(e) = self.try_open(&name, Some(path), result.file.bytes, result.password.as_deref(), true) {
                        message.push_str(&format!("; saved successfully, but could not open: {e}"));
                    }
                }
                self.notify(message);
                return;
            }
            Err(e) => {
                self.notify_error(e);
                return;
            }
        };
        let read = found.iter().filter(|p| p.skipped.is_none()).count();
        let skipped = found.len() - read;
        match self.session.apply_ocr(doc, &found) {
            Ok(words) => {
                if let Some(info) = self.session.get(doc).map(|d| d.info.clone())
                    && let Some(view) = self.views.iter_mut().find(|v| v.id == doc)
                {
                    view.document_changed(&info);
                }
                let words_part = if words == 1 {
                    crate::i18n::fmt(tl!("Recognized 1 word"), &[])
                } else {
                    crate::i18n::fmt(tl!("Recognized {n} words"), &[("n", &words.to_string())])
                };
                let pages_part = if read == 1 { tl!("1 page").to_string() } else { crate::i18n::fmt(tl!("{n} pages"), &[("n", &read.to_string())]) };
                let mut msg = crate::i18n::fmt(tl!("{words} on {pages}"), &[("words", &words_part), ("pages", &pages_part)]);
                if skipped > 0 {
                    msg.push_str(&{
                        format!(
                            "; skipped page(s): {}",
                            found
                                .iter()
                                .filter_map(|p| p.skipped.as_ref().map(|why| format!("{}: {why}", p.page + 1)))
                                .collect::<Vec<_>>()
                                .join("; ")
                        )
                    });
                }
                self.notify(msg);
            }
            Err(e) => self.notify_error(e),
        }
    }
}

fn update_progress(p: &Arc<Mutex<OcrProgress>>, done: usize, total: usize) -> bool {
    let Ok(mut s) = p.lock() else { return false };
    s.done = done;
    s.total = total;
    !s.cancel
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::{Harness, kittest::Queryable};

    #[test]
    fn ocr_copy_progress_cancel_keeps_source_and_clears_progress() {
        let mut h = Harness::builder().with_size(egui::vec2(1200.0, 900.0)).build_eframe(|_| {
            let mut app = PdfCraftApp::new();
            app.set_option("language", "en").unwrap();
            app.open_bytes("notes.txt", None, b"Source remains unchanged".to_vec()).unwrap();
            let id = app.active_ids().unwrap().1;
            app.ocr_run = Some(OcrRun { doc: id, progress: Arc::new(Mutex::new(OcrProgress { done: 1, total: 4, ..Default::default() })) });
            app
        });
        h.run_steps(3);
        let id = h.state().active_ids().unwrap().1;
        let bytes = h.state().session.get(id).unwrap().bytes.clone();
        let dirty = h.state().session.get(id).unwrap().dirty;
        let undo = h.state().session.get(id).unwrap().can_undo().map(str::to_owned);
        let p = h.state().ocr_run.as_ref().unwrap().progress.clone();
        h.get_by_label("Cancel").click();
        h.run_steps(2);
        assert!(p.lock().unwrap().cancel);
        h.get_by_label("Cancelling…");
        p.lock().unwrap().result = Some(Err("Text recognition cancelled; no copy was saved".into()));
        h.run_steps(2);
        assert!(h.state().ocr_run.is_none() && h.state().progress_notice.is_none());
        let source = h.state().session.get(id).unwrap();
        assert_eq!(source.bytes, bytes);
        assert_eq!(source.dirty, dirty);
        assert_eq!(source.can_undo(), undo.as_deref());
    }
}
