//! Detached searchable-copy jobs. No operation here mutates the originating session.

use std::sync::Arc;

use pdfcraft_cos::{Dict, Object, Stream};
use pdfcraft_render::{PageRenderer, RenderConfig, RenderRequest, RequestKind};

use super::{FileResult, Ocr, OcrJob, OcrPage, OcrSettings, PlacedWord};
use crate::{DocId, Session};

pub const DERIVATIVE_WARNING: &str = "This creates an unencrypted raster copy. Original passwords and restrictions are not retained. Vectors, bookmarks, links, forms, layers and signatures are lost; only their rendered appearance is retained inside the crop box. Confirm that you are authorized to create this searchable derivative, or reopen with the owner password for a faithful encrypted copy.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyKind {
    Faithful,
    Raster,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CopyPolicy {
    pub kind: CopyKind,
    /// The maximum resolution of a print-authorized derivative. Low-quality printing is
    /// conservatively limited to 150 dpi; a requested higher resolution is refused.
    pub max_dpi: f32,
    pub warnings: Vec<String>,
}

/// Decide rights independently. Modification permits adding text while keeping the exact
/// encryption handler. Print rights alone permit only an explicitly authorized derivative.
/// Copy/accessibility rights do not imply modification or high-quality printing rights.
pub fn copy_policy(permissions: Option<pdfcraft_cos::Permissions>) -> Result<CopyPolicy, String> {
    let Some(p) = permissions else {
        return Ok(CopyPolicy { kind: CopyKind::Faithful, max_dpi: 600.0, warnings: Vec::new() });
    };
    if p.modify() {
        return Ok(CopyPolicy {
            kind: CopyKind::Faithful,
            max_dpi: 600.0,
            warnings: vec!["The copy retains the source encryption and permissions.".into()],
        });
    }
    if p.print() {
        let mut warnings = vec![DERIVATIVE_WARNING.into()];
        if !p.copy() {
            warnings.push("Content copying is restricted. Searchable derivative export requires your explicit authorization.".into());
        }
        return Ok(CopyPolicy { kind: CopyKind::Raster, max_dpi: if p.print_high_quality() { 600.0 } else { 150.0 }, warnings });
    }
    Err(format!(
        "Searchable-copy export requires modification permission or authorized printing. This document prohibits both. Copying: {}; accessibility extraction: {}. Reopen with the owner password. No output will be written.",
        if p.copy() { "allowed" } else { "restricted" },
        if p.extract_for_accessibility() { "allowed (does not authorize disk export)" } else { "restricted" },
    ))
}

pub fn suggested_copy_name(name: &str) -> String {
    let name = std::path::Path::new(name).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "document".into());
    let stem = if name.to_ascii_lowercase().ends_with(".pdf") { name.get(..name.len().saturating_sub(4)).unwrap_or("document") } else { &name };
    format!("{stem}_OCR.pdf")
}

pub struct CopyResult {
    pub file: FileResult,
    pub kind: CopyKind,
    pub warnings: Vec<String>,
    /// Used only to reopen a faithful encrypted copy; never included in diagnostics.
    pub password: Option<String>,
}

pub struct CopyJob {
    name: String,
    source_path: Option<String>,
    recognition: OcrJob,
    source: pdfcraft_cos::Document,
    password: Option<String>,
    policy: CopyPolicy,
}

impl Session {
    pub fn ocr_copy_policy(&self, id: DocId) -> Result<CopyPolicy, String> {
        let doc = self.get(id).ok_or("no such document")?;
        // A missing editor is not an unprotected document: fail closed.
        let editor =
            doc.editor.as_ref().ok_or_else(|| doc.read_only_reason.clone().unwrap_or_else(|| "The source structure cannot be read safely".into()))?;
        copy_policy(editor.cos.permissions())
    }

    /// Snapshot the working PDF without saving, rebasing, editing or changing source history.
    /// `authorize_derivative` acknowledges the security/fidelity loss in [`DERIVATIVE_WARNING].
    pub fn ocr_copy_job(&self, id: DocId, pages: &[usize], settings: OcrSettings, authorize_derivative: bool) -> Result<CopyJob, String> {
        let doc = self.get(id).ok_or("no such document")?;
        let policy = self.ocr_copy_policy(id)?;
        if !settings.dpi.is_finite() || !(72.0..=policy.max_dpi).contains(&settings.dpi) {
            return Err(format!("OCR resolution must be between 72 and {} dpi for this document", policy.max_dpi));
        }
        if !super::LANGUAGES.iter().any(|l| l.0 == settings.language) {
            return Err(format!("Unsupported OCR language {:?}; the installed engine supports English/Latin text, not Chinese", settings.language));
        }
        if pages.iter().any(|p| *p >= doc.info.pages.len()) {
            return Err("OCR page range contains a page outside the document".into());
        }
        if policy.kind == CopyKind::Raster && !authorize_derivative {
            return Err(DERIVATIVE_WARNING.into());
        }
        let editor = doc.editor.as_ref().ok_or("The source structure cannot be read safely")?;
        let recognition = self.ocr_job(id, pages, settings).ok_or("The source cannot be read")?;
        Ok(CopyJob {
            name: doc.name.clone(),
            source_path: doc.path.clone(),
            recognition,
            source: editor.cos.clone(),
            password: editor.keys.reopen.clone(),
            policy,
        })
    }
}

impl CopyJob {
    pub fn source_path(&self) -> Option<&str> {
        self.source_path.as_deref()
    }
    pub fn page_count(&self) -> usize {
        self.recognition.pages.len()
    }

    /// All output is built independently; cancelling at any stage discards it. The caller
    /// publishes it atomically only after this succeeds and a final cancellation check.
    pub fn run(self, ocr: &Ocr, mut progress: impl FnMut(usize, usize) -> bool) -> Result<CopyResult, String> {
        crate::guard(|| self.run_inner(ocr, &mut progress))?
    }

    fn run_inner(self, ocr: &Ocr, progress: &mut impl FnMut(usize, usize) -> bool) -> Result<CopyResult, String> {
        let Self { name, recognition, source, password, policy, .. } = self;
        let total = recognition.pages.len().saturating_add(if policy.kind == CopyKind::Raster { recognition.infos.len() } else { 0 });
        let mut out = Session::new();
        let (bytes, reopen, preserved) = if policy.kind == CopyKind::Raster {
            let (bytes, text) = raster_copy(&source, &recognition, |done, _| progress(done, total))?;
            (bytes, None, text)
        } else {
            (recognition.bytes.clone(), password.clone(), Vec::new())
        };
        let offset = if policy.kind == CopyKind::Raster { recognition.infos.len() } else { 0 };
        let pages = recognition.run_checked(ocr, |done, _| progress(done + offset, total))?;
        if let Some(p) = pages.iter().find(|p| p.skipped.as_ref().is_some_and(|s| s.starts_with("error:"))) {
            return Err(format!("Page {}: {}; no copy was saved", p.page + 1, p.skipped.as_deref().unwrap_or("recognition failed")));
        }
        let id = out.open(name, None, bytes, reopen.as_deref()).map_err(|e| e.to_string())?;
        if !preserved.is_empty() {
            out.apply_ocr(id, &preserved).map_err(|e| e.to_string())?;
        }
        out.apply_ocr(id, &pages).map_err(|e| e.to_string())?;
        let bytes = out.save_bytes(id).map_err(|e| e.to_string())?;
        if !progress(total, total) {
            return Err("Text recognition cancelled; no copy was saved".into());
        }
        Ok(CopyResult { file: FileResult { bytes, pages }, kind: policy.kind, warnings: policy.warnings, password: reopen })
    }
}

/// Render each visible crop losslessly into a fresh object graph. Image coordinates and text
/// share the source page's mapping, including nonzero crop origins, rotation and UserUnit.
fn raster_copy(
    source: &pdfcraft_cos::Document,
    job: &OcrJob,
    mut progress: impl FnMut(usize, usize) -> bool,
) -> Result<(Arc<Vec<u8>>, Vec<OcrPage>), String> {
    let mut doc = pdfcraft_create::blank(612.0, 792.0, job.infos.len()).map_err(|e| e.to_string())?;
    let targets = pdfcraft_organize::pages(&doc).map_err(|e| e.to_string())?;
    let sources = pdfcraft_organize::pages(source).map_err(|e| e.to_string())?;
    let boxes = pdfcraft_organize::page_boxes(source).map_err(|e| e.to_string())?;
    let mut renderer = PageRenderer::new(
        job.bytes.clone(),
        RenderConfig { password: job.password.as_deref().map(Arc::from), reject_oversize: true, ..Default::default() },
    );
    let mut preserved = Vec::new();
    let mut compressed_bytes = 0usize;
    for (page, info) in job.infos.iter().enumerate() {
        if !progress(page, job.infos.len()) {
            return Err("Text recognition cancelled; no copy was saved".into());
        }
        let shot = renderer.render(RenderRequest { page, scale: job.settings.dpi / 72.0, ..Default::default() });
        if let Some(e) = shot.error {
            return Err(format!("Page {} could not be rendered: {e}", page + 1));
        }
        let rgb: Vec<u8> = shot.rgba.as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
        let mut image = Dict::new();
        image.set(b"Type".to_vec(), Object::name("XObject"));
        image.set(b"Subtype".to_vec(), Object::name("Image"));
        image.set(b"Width".to_vec(), Object::Int(shot.width.into()));
        image.set(b"Height".to_vec(), Object::Int(shot.height.into()));
        image.set(b"ColorSpace".to_vec(), Object::name("DeviceRGB"));
        image.set(b"BitsPerComponent".to_vec(), Object::Int(8));
        let image = Stream::flate(image, &rgb);
        compressed_bytes = compressed_bytes.saturating_add(image.raw.len());
        if compressed_bytes > 512 * 1024 * 1024 {
            return Err(
                "The raster copy exceeds the 512 MiB output budget. Choose a lower resolution or reopen with the owner password for a faithful copy."
                    .into(),
            );
        }
        let image = doc.add(Object::Stream(image));
        let o = info.view_to_user(0.0, info.height);
        let br = info.view_to_user(info.width, info.height);
        let tl = info.view_to_user(0.0, 0.0);
        let matrix = [br[0] - o[0], br[1] - o[1], tl[0] - o[0], tl[1] - o[1], o[0], o[1]];
        let content = format!("q {} cm /Im0 Do Q\n", matrix.iter().map(|n| format!("{n:.6}")).collect::<Vec<_>>().join(" "));
        let stream = doc.add(Object::Stream(Stream::flate(Dict::new(), content.as_bytes())));
        let mut xobjects = Dict::new();
        xobjects.set(b"Im0".to_vec(), Object::Ref(image));
        let mut resources = Dict::new();
        resources.set(b"XObject".to_vec(), Object::Dict(xobjects));
        let target = targets.get(page).ok_or("Missing output page")?.obj;
        let page_boxes = boxes.get(page).ok_or("Missing source page boxes")?;
        let src = sources.get(page).ok_or("Missing source page")?;
        let user_unit = source.get(src.obj).as_dict().and_then(|d| d.get(b"UserUnit")).map(|v| source.resolve(v));
        doc.update_dict(target, |d| {
            for (key, rect) in [b"MediaBox".as_slice(), b"CropBox", b"BleedBox", b"TrimBox", b"ArtBox"].into_iter().zip(page_boxes) {
                d.set(key.to_vec(), Object::Array(rect.iter().map(|v| Object::Real(*v)).collect()));
            }
            d.set(b"Rotate".to_vec(), Object::Int(info.rotation.into()));
            if let Some(v) = user_unit {
                d.set(b"UserUnit".to_vec(), (*v).clone());
            }
            d.set(b"Resources".to_vec(), Object::Dict(resources));
            d.set(b"Contents".to_vec(), Object::Ref(stream));
        })
        .map_err(|e| e.to_string())?;
        // Retain selectable text on skipped or unselected pages when it can be represented
        // accurately by the current Latin text-layer writer. Never silently substitute CJK.
        let keep_text = !job.pages.contains(&page) || job.settings.skip_text_pages;
        if keep_text {
            let text = renderer.render(RenderRequest { page, kind: RequestKind::Text, ..Default::default() });
            if let Some(e) = text.error {
                return Err(format!("Page {} text could not be read: {e}", page + 1));
            }
            if let Some(text) = text.text {
                let mut words = Vec::new();
                for glyph in &text.glyphs {
                    if glyph.text.trim().is_empty() {
                        continue;
                    }
                    if pdfcraft_fonts::first_non_win_ansi(&glyph.text).is_some() {
                        return Err(format!(
                            "Page {} contains text outside the Latin text-layer encoding. Reopen with the owner password to preserve its text in a faithful copy.",
                            page + 1
                        ));
                    }
                    words.push(PlacedWord::place(&pdfcraft_ocr::Word { text: glyph.text.clone(), rect: glyph.rect }, |x, y| {
                        info.view_to_user(x, y).map(f64::from)
                    }));
                }
                if !words.is_empty() {
                    preserved.push(OcrPage { page, words, already_on_page: 0, skipped: None });
                }
            }
        }
    }
    let bytes = pdfcraft_cos::write_full(&doc, &Default::default()).map_err(|e| e.to_string())?;
    Ok((Arc::new(bytes), preserved))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ocr_copy_permissions_are_independent_and_fail_closed() {
        let permissions = |bits, owner| Some(pdfcraft_cos::Permissions { bits, owner });
        assert_eq!(copy_policy(None).unwrap().kind, CopyKind::Faithful);
        assert_eq!(copy_policy(permissions(0, true)).unwrap().kind, CopyKind::Faithful);
        assert_eq!(copy_policy(permissions(1 << 3, false)).unwrap().kind, CopyKind::Faithful);
        assert_eq!(copy_policy(permissions((1 << 2) | (1 << 11), false)).unwrap().max_dpi, 600.0);
        assert_eq!(copy_policy(permissions(1 << 2, false)).unwrap().max_dpi, 150.0);
        for bits in [0, 1 << 4, 1 << 9, (1 << 4) | (1 << 9)] {
            assert!(copy_policy(permissions(bits, false)).unwrap_err().contains("owner password"));
        }
        assert_eq!(suggested_copy_name("Report.PDF"), "Report_OCR.pdf");
    }

    #[test]
    fn ocr_copy_raster_preserves_crop_rotation_user_unit_and_appearance() {
        let mut source = pdfcraft_create::blank(200.0, 300.0, 4).unwrap();
        let pages = pdfcraft_organize::pages(&source).unwrap();
        for (i, page) in pages.iter().enumerate() {
            let stream = source.add(Object::Stream(Stream::flate(Dict::new(), b"1 0 0 rg 25 35 70 90 re f 0 0 1 rg 100 150 40 60 re f")));
            source
                .update_dict(page.obj, |d| {
                    d.set(b"Contents".to_vec(), Object::Ref(stream));
                    d.set(b"Rotate".to_vec(), Object::Int((i * 90) as i64));
                    d.set(b"CropBox".to_vec(), Object::Array(vec![10.into(), 20.into(), 190.into(), 280.into()]));
                    d.set(b"UserUnit".to_vec(), Object::Real(1.25 + i as f64 * 0.25));
                })
                .unwrap();
        }
        let bytes = Arc::new(pdfcraft_cos::write_full(&source, &Default::default()).unwrap());
        let mut s = Session::new();
        let id = s.open("geometry.pdf", None, bytes.clone(), None).unwrap();
        let job = s.ocr_job(id, &[], OcrSettings { dpi: 72.0, ..Default::default() }).unwrap();
        let infos = job.infos.clone();
        let (copy, text) = raster_copy(&source, &job, |_, _| true).unwrap();
        assert!(text.is_empty());
        let output = pdfcraft_render::inspect(copy.clone(), None).unwrap();
        for (a, b) in output.pages.iter().zip(&infos) {
            assert_eq!((a.width, a.height, a.crop, a.rotation), (b.width, b.height, b.crop, b.rotation));
        }
        let mut before = PageRenderer::new(bytes, Default::default());
        let mut after = PageRenderer::new(copy, Default::default());
        for page in 0..4 {
            let request = RenderRequest { page, scale: 1.0, ..Default::default() };
            let a = before.render(request);
            let b = after.render(request);
            assert_eq!((a.width, a.height), (b.width, b.height));
            let error: u64 = a.rgba.iter().zip(b.rgba.iter()).map(|(a, b)| a.abs_diff(*b) as u64).sum();
            assert!(error as f64 / (a.rgba.len() as f64) < 1.0, "page {page} differs: {error}");
        }
    }
}
