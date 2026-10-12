# Searchable-copy implementation and validation

Implemented on Windows with the existing Rust OCR engine and pinned models. No OCR dependency was added. The fork also includes the contributor logo replacement and Discord removal. The original implementation results below are followed by the full upstream-sync validation.

## Implementation files

| Files | Change |
| --- | --- |
| `crates/engine/src/ocr/copy.rs`, `ocr.rs` | Detached job, permission policy, faithful encrypted copy or authorized raster derivative, checked cancellation, accurate page transforms. |
| `crates/engine/src/commands.rs`, `catalog.rs` | Copy command and reachable OCR dialog for protected documents. |
| `crates/platform/src/staging.rs` | Atomic, collision-safe publication, source-path refusal, cancellation cleanup. |
| `crates/ui-egui/src/ocr_ui.rs`, `dialogs.rs`, `commands.rs`, `lib.rs`, `optimize_ui.rs` | Two modes, authorization and destination options, background progress/cancel, separate output tab. |
| `crates/ui-egui/src/i18n/{bg,cs,de,es,fr,hu,it,ja,pt-br,ru,te,uk,zh-hans,zh-hant}.tsv` | New command and catalogue translations in every bundled language. |
| `crates/automation/src/a11y.rs`, `tools.rs`, `lib.rs` | Shared CLI/MCP tool, strict DPI and filesystem validation, safe batch destinations. |
| `crates/engine/src/tests.rs`, `crates/ui-egui/tests/ocr.rs`, `crates/automation/tests/automation.rs`, `apps/pdfcraft-cli/tests/ocr_copy.rs` | Synthetic protected scans, genuine recognition and search, unchanged source/history, positioning, UI, MCP and CLI checks. |
| `crates/automation/tests/hostile_args.rs` | Test an escaping symlink only when the host actually permits creating it. |
| `parity/acrobat-features.toml` | Feature and test evidence; merged duplicate existing field-panel evidence/notes that prevented TOML parsing. |
| Engine/automation READMEs, `docs/ocr-searchable-copy.md`, `ROADMAP.md` | API, usage, security decisions, limitations and session log. |

## Protected RC4 example

After user-password authentication, denied modification/assembly/copying with high-resolution printing allowed selects the raster route. Accessibility permission alone does not authorize disk export. The user must confirm authorization for the unencrypted derivative. Original encrypted objects are never edited, and the source retains its password and permission bits. Owner authentication instead enables a faithful encrypted copy with the original structure and restrictions.

The independent CLI verification used a synthetic RC4 128-bit scanned PDF with exactly those restrictions. The raster copy contained seven recognized words and a successful search. Before and after source SHA-256 was `847C9E35B803242EE2981003466F785F1CA47D67FE918399EF9A6DF310BF7069`. A second copy made with the owner password reopened with the original reader password and retained RC4 128-bit encryption, denied copying and denied modification.

## Usage

1. Open the document and authenticate normally.
2. Choose **Scan & OCR → In this file → Recognize text** or **Save as New Searchable PDF**.
3. Select **Save as New Searchable PDF**, pages, English, resolution and whether to skip existing text.
4. For print-only sources, review the warning and confirm derivative authorization.
5. Choose whether to open the result in a new tab. Click **Recognize text**, then choose the destination suggested as `<original>_OCR.pdf`.

Existing output requires the explicit replacement checkbox; choosing the source is always refused. See [the usage guide](ocr-searchable-copy.md) for the shared CLI/MCP `ocr_create_searchable_copy` arguments and a complete script.

## Verification

Commands use `+stable-x86_64-pc-windows-msvc` because this machine's default GNU toolchain lacks `dlltool`. Existing OCR models were downloaded with `cargo +stable-x86_64-pc-windows-msvc run --locked -p xtask -- models` and their pinned hashes verified, so OCR assertions ran against the real models.

| Command | Actual result |
| --- | --- |
| `cargo +stable-x86_64-pc-windows-msvc fmt --all --check` | Passed. |
| `git diff --check` | Passed. |
| `cargo +stable-x86_64-pc-windows-msvc clippy --locked --offline --workspace --all-targets -j4 -- -D warnings` | Passed with warnings denied across every workspace target. |
| `cargo +stable-x86_64-pc-windows-msvc test --locked --offline -p pdfcraft-ui-egui --test ocr -j4` | All four passed: in-place dialog, batch, restricted copy dialog, separate-tab copy. |
| `cargo +stable-x86_64-pc-windows-msvc test --locked --offline -p pdfcraft-automation --test hostile_args -j4` | Both passed: confinement and hostile arguments. |
| `cargo +stable-x86_64-pc-windows-msvc test --locked --offline -p pdfcraft-cli --test ocr_copy -j4` | Passed: actual CLI copy, search hit, unchanged original and collision refusal. |
| `cargo +stable-x86_64-pc-windows-msvc test --locked --offline --workspace --lib -j4` | Passed: 997 tests across 30 suites, two existing ignored tests; includes all 128 engine tests and all 184 UI library tests. |
| `cargo +stable-x86_64-pc-windows-msvc test --locked --offline -p pdfcraft-ui-egui --test ocr --test commands --test control --test protect --test ui -j4` | All 73 passed: 12 commands, 23 control-channel, four OCR, two protection and 32 main UI tests. |
| `cargo +stable-x86_64-pc-windows-msvc run --locked --offline -p xtask -- parity --partial` | Passed. Reports existing project parity gaps, as expected for this partial gate. |
| `cargo +stable-x86_64-pc-windows-msvc run --locked --offline -p xtask -- assets` | Passed: 252 repository assets, six dependency assets and 22 fetched assets. |
| `cargo +stable-x86_64-pc-windows-msvc run --locked --offline -p xtask -- layers` | Passed: 34 crates, no layering violations. |
| `cargo +stable-x86_64-pc-windows-msvc build --locked --offline -p pdfcraft -j4` | Passed. |
| `cargo +stable-x86_64-pc-windows-msvc check --locked -p pdfcraft-ui-egui --target wasm32-unknown-unknown` | Blocked: this machine lacks target `core`/`std` (E0463). Two `rustup target add wasm32-unknown-unknown --toolchain stable-x86_64-pc-windows-msvc` attempts failed while downloading rust-std; no web-pass claim. |

The rebuilt native app was launched with opt-in `--control` against the synthetic protected PDF. The live screenshot showed all settings and buttons; the copy option was suggested, in-place editing disabled, and the OCR button enabled only after derivative authorization. Test captures stay in ignored `target/ocr-native-check/`; no control credentials or synthetic PDF binaries were added to the repository.

Windows did not permit the hostile-arguments fixture to create a symbolic link. Its symlink case therefore skips honestly; traversal and absolute-path checks still run. A separate live CLI check created a Windows directory junction inside a restricted root pointing outside it: `ocr_create_searchable_copy` refused `link-out/escape_OCR.pdf` as outside `--root`, and no output appeared. The temporary junction was then removed without touching its target.

An initial `cargo +stable-x86_64-pc-windows-msvc test --locked --offline --workspace -j4` exposed confinement-test setup/batch error handling, then missing new command translations and a cancellation fixture assuming imported text started clean. These were corrected. The final validation scope is every workspace library suite, the five named UI integration suites, CLI OCR and the full automation integration suite (100 passing tests, including the new tool and MCP test), plus hostile-argument checks. The entire workspace integration set was not rerun after the final localization fix; no complete all-integration-suite pass is claimed. Clippy was rerun across all targets after the final source changes.

## Limits

English/Latin without accents only; Chinese is unsupported. Raster derivatives lose vector fidelity, off-crop content, bookmarks, links, forms, layers and signatures. They preserve effective page boxes, rotation, UserUnit and visible appearance using lossless image compression. Automatic orientation correction and deskew are not added. Unsupported preserved-text encodings fail explicitly.

Print and modification both denied: refuse export and request owner authentication; no new accessibility-only in-session mode. Low-quality printing caps derivatives at 150 dpi. Renderer size caps and a 512 MiB compressed-image budget apply. Cancellation waits for the current page's inference to finish, then discards unpublished output. CLI/MCP remain synchronous without a new cancellation protocol. Atomic no-overwrite publication requires filesystem hard-link support; unsupported filesystems fail safely. Browser packaging/workers retain existing limitations.

## Single-executable OCR build (2026-10-10)

The optional `embedded-ocr` application feature embeds the same unmodified, SHA-256-pinned
model bytes and their attribution/full licences. rten loads static memory directly; there is
no runtime model download or extraction. About exposes the model licence texts in a bounded
scroll area. Ordinary builds retain external-model discovery and existing package layouts.

The detection and recognition SHA-256 values matched `ATTRIBUTION.toml`, as did both model
licence hashes. Refreshed the attribution hashes for the 14 contributor-edited translation
catalogues from the earlier searchable-copy work, then regenerated `ATTRIBUTION.md`; the
asset audit passed (252 repository, six dependency, 22 fetched assets).

| Command (MSVC toolchain, `--locked --offline`, `-j4`) | Result |
| --- | --- |
| `cargo test -p pdfcraft-ocr -p pdfcraft-engine --lib --features pdfcraft-engine/embedded-ocr` | 8 OCR and 128 engine tests passed; one existing timing probe ignored. Direct embedded recognition tests never search installed/source-tree files, and assert no model extraction. Corrupt explicit overrides fail safely. |
| `cargo test -p pdfcraft-ui-egui --features embedded-ocr --test links --test ocr` | All 10 passed, including readable embedded licences, existing branding/links and all four OCR flows. |
| `cargo test -p pdfcraft-cli --features embedded-ocr --test ocr_copy` | Passed from a copied executable, a different working directory and a nonexistent model override. Real OCR, search hit, unchanged source, collision refusal and no extracted model folder verified. |
| `cargo build -p pdfcraft -p pdfcraft-cli --features pdfcraft/embedded-ocr,pdfcraft-cli/embedded-ocr` | Passed. The delivered desktop EXE is 87,734,784 bytes and contains both models. |
| `cargo clippy --workspace --all-targets --features pdfcraft/embedded-ocr,pdfcraft-cli/embedded-ocr -- -D warnings` | Passed with warnings denied. |
| `cargo test -p pdfcraft-ocr --lib` | All six tests passed with embedding disabled, preserving external-model support. |
| Formatting, whitespace, assets and layering gates | Passed. |

The final app and CLI binaries were copied to an isolated verification directory without
models beside them and with `PDFCRAFT_MODELS` pointing to a nonexistent directory. The final
CLI created an authorized searchable derivative, found the word `derivative`, left the
encrypted source byte-for-byte unchanged and extracted no models. The final native app was
launched with opt-in control and system fonts disabled; its About model attribution and full
licence panel were exercised and the actual screenshot inspected. The source-tree models
were left in place; independence from all model directories is additionally established by
the direct in-memory recognition tests, which never invoke filesystem discovery.

## Upstream 0.6.0 integration (2026-10-12)

Integrated upstream main through `84a75eae1b46d155656f10e9278373726a808fbb`,
258 commits beyond the original fork baseline. Preserved the contributor logo,
Discord removal, protected searchable-copy workflow and optional embedded models.
Both model loaders now use upstream's trained alphabet (including the euro sign),
and upstream's hybrid-page filtering remains available without duplicating existing
text. Copy controls use upstream's widgets; all 20 catalogs include the copy command.
The new Windows document icon uses the same contributor emblem and licence.

All commands below used `+stable-x86_64-pc-windows-msvc` (Rust 1.96.1) and `--locked`.

| Command | Actual result |
| --- | --- |
| `cargo fmt --all --check` and `git diff --check` | Passed. |
| `cargo clippy --workspace --all-targets --features pdfcraft/embedded-ocr,pdfcraft-cli/embedded-ocr -j4 -- -D warnings` | Passed. Three upstream lint expressions were simplified without changing behavior. |
| `cargo test --workspace --features pdfcraft/embedded-ocr,pdfcraft-cli/embedded-ocr -j4 -- --test-threads=4` | Full workspace passed: 2,663 tests, 13 existing ignored tests, 149 suites including doctests. Real OCR, embedded recognition, source preservation, copy exports, engine, CLI/MCP and UI regressions ran. |
| `cargo check --target wasm32-unknown-unknown -j4` with `-p` for every library package under `crates/` | All 31 library crates passed. Used a separate `target/upstream-wasm` directory; existing upstream warnings remain. |
| `cargo xtask assets --write`, then `cargo xtask assets` | Passed: 262 repository assets, six bundled dependency assets, 22 fetched assets. Generated attribution is current. |
| `cargo xtask layers` | Passed: 35 crates, no layering violations. |
| `cargo xtask parity` | Passed; existing upstream missing-tool notices remain. |

The first full test attempt exposed a host filesystem issue: Windows AF_UNIX sockets
bound under the normal C-drive temporary directory could not be connected to.
A minimal local probe reproduced the problem independently of PdfCraft; the same
probe succeeded on D:. Setting process-local `TMP` and `TEMP` to
`target/upstream-test-temp` allowed all eight single-instance tests and then the
entire workspace suite to pass. No tests were removed or suppressed. Optional
craft-fonts input was absent; its existing optional checks retain their documented
behavior.

The WebAssembly standard-library target was installed successfully before the full
31-crate check, superseding the blocked web check from the original implementation.
The rebuilt 0.6.0 desktop app was copied to an isolated directory, launched against
a synthetic scan with opt-in control, system fonts disabled and a nonexistent model
override. Inspected live screenshots of About, full embedded OCR licences and the
searchable-copy dialog. The emblem, settings and upstream hybrid-page option were
visible; the app exited normally. Screenshots, fixtures and logs remain in ignored
`target/`; the temporary control credential file was removed.
