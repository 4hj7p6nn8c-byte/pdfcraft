# Searchable OCR copies

Open a PDF and choose **Scan & OCR → In this file → Recognize text**, or **Save as New Searchable PDF** in the same tool panel. The dialog offers two modes:

- **Modify Current PDF** adds an invisible text layer as one undoable edit. Modification permission is required.
- **Save as New Searchable PDF** captures the working PDF independently, suggests `<name>_OCR.pdf`, and saves to the selected destination. The source file, dirty flag, undo/redo history and open tab are unchanged. Already committed unsaved edits are included in the snapshot; uncommitted typing in other UI tools is not committed by OCR.

Choose all/current/range pages, English, resolution and whether to skip pages with selectable text. The page setting selects which pages undergo recognition; every source page remains in the copy. Enable **Open the copy in a new tab** if wanted. Desktop work runs on a background thread, with a progress card and Cancel button. Cancellation is checked between pages, after recognition and during the staged save, and discards unpublished output. A running individual OCR inference cannot be interrupted mid-page.

Existing destinations are refused unless **Allow replacing an existing destination** is checked. A source path or its symlink alias can never be replaced, even with that checkbox. Cancelling the destination picker starts no work. Saves stage beside the destination, sync and close the file, then publish atomically. The no-overwrite publication uses a hard link so a racing filename collision cannot replace another file; filesystems without hard-link support return an error and leave the destination untouched.

## Permissions and output security

| Authenticated source | Export behavior |
| --- | --- |
| Unprotected, owner authenticated, or modification allowed | Faithful object-level copy; adds invisible text in an independent session and retains encryption, permission bits, existing text, images, unknown objects and interactive structure. Existing image streams are not recompressed. |
| Modification prohibited; high-quality printing allowed | Raster derivative at 72–600 dpi, only after explicit authorization. |
| Modification prohibited; low-quality printing allowed | Raster derivative at 72–150 dpi, only after explicit authorization. Higher DPI is refused. |
| Modification and printing prohibited | Export refused. Copying and accessibility extraction are evaluated separately and reported; neither is treated as authorization to edit or export a searchable derivative. Reopen with the owner password. |
| Incorrect/missing password or unreadable source structure | Existing authentication/opening error; no export. |

For the RC4 128-bit example (user authentication, no editing/assembly/copying, high-quality printing and accessibility permitted), choose copy mode and confirm **I am authorized to create this unencrypted searchable derivative**. The app renders an independently built PDF and adds the recognized words there. It never edits protected source objects or its encryption dictionary. The resulting derivative is unencrypted and does not retain restrictions; this is disclosed before confirmation and in the completion report. To retain the original security and structure, reopen using the owner password and use the faithful route. Merely knowing a user password does not count as owner authentication.

Raster copies retain effective Media/Crop/Bleed/Trim/Art boxes, page rotation, UserUnit, mixed dimensions, appearance inside the visible crop, and OCR coordinates in original user space. Rendered images are compressed losslessly with Flate. They lose vector fidelity, live forms, links, bookmarks, layers and signatures. Off-crop content is absent. Selectable text on skipped/unselected pages is reconstructed as invisible text when the current Latin encoding supports it; unsupported text causes a clear refusal rather than silent substitution. Selecting **do not skip** runs OCR on selected text pages instead of duplicating their original layer. Renderer fidelity remains that of PdfCraft's existing renderer; the derivative is not an exact substitute for a vector original.

## CLI and MCP

Both expose the same `ocr_create_searchable_copy` tool. Pages are 1-based; output paths obey `--root`. Open protected files through the existing `doc_open` password argument. Example `steps.json`:

```json
[
  {"tool":"doc_open","args":{"path":"scan.pdf","password":"reader-password"}},
  {"tool":"ocr_create_searchable_copy","args":{
    "doc":1,"path":"scan_OCR.pdf","pages":[1,2],"language":"en",
    "dpi":300,"skip_text_pages":true,"authorize_derivative":true,
    "overwrite":false,"open":true
  }},
  {"tool":"text_find","args":{"doc":2,"query":"recognized word"}}
]
```

```text
pdfcraft-cli run --root <directory> --script steps.json
pdfcraft-cli mcp --root <directory>
```

MCP: call `ocr_create_searchable_copy` with the same arguments. The response includes `output`, optional newly opened `doc`, `source`, `kind` (`faithful`/`raster`), recognized `words`, per-page counts/skips and security/fidelity `warnings`; passwords are never returned. CLI/MCP calls are synchronous, as with other exports; this feature does not add MCP cancellation support. The desktop cancellation pipeline is shared with the engine's checked job and atomic writer.

## Limits

- Source builds install the pinned existing models with `cargo xtask models`; no new engine is added. Model files remain ignored build inputs.
- Single-executable builds can include those models and their licences with `cargo build -p pdfcraft --features embedded-ocr` after fetching them. OCR reads them directly from the EXE without a model folder, extraction or runtime downloads. See `crates/ocr/README.md`.
- Current models support English/Latin text without accents. Chinese and other scripts are not supported; recognition is imperfect and needs review. The UI says so, and unsupported language arguments fail.
- Copy recognition/render errors abort publication and identify the failing page. No fabricated text or partially recognized export is saved. Blank pages may legitimately yield zero words.
- Page rotation metadata is preserved; this does not add automatic orientation detection, deskew, or correction of sideways/upside-down scan content.
- Renderer page-size limits are enforced without silently lowering raster output DPI. Raster images have a 512 MiB cumulative compressed-data budget; exceeding it requires a lower resolution or faithful output.
- Signed/certified PDFs retain the existing engine's edit/signature restrictions for the faithful route; this does not promise preservation of signature validity after adding text. Raster derivatives retain no signatures.
- When both printing and modification are prohibited, no new read-only in-session OCR mode is provided. Accessibility rights are reported but do not enable a disk export.
- Web model packaging/background workers remain limited by the existing browser build. A browser copy downloads through the existing download API; the desktop destination picker and filesystem guarantees apply to native builds.
