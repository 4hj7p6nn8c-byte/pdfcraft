# pdfcraft-ocr

Latin text recognition through the existing ocrs/rten engine, plus invisible PDF text-layer
generation. `Ocr::find()` loads one recognizer; the engine facade caches and shares it.

Ordinary builds find external models using `PDFCRAFT_MODELS`, the installed package layouts,
then the source tree's ignored `assets/models/` directory. Fetch the licensed, hash-pinned
models and licence texts using `cargo xtask models`.

For a single executable, fetch the models first, then enable `embedded-ocr` on the desktop
or CLI application:

```powershell
cargo xtask models
cargo build --locked -p pdfcraft --features embedded-ocr
cargo build --locked -p pdfcraft-cli --features embedded-ocr
```

This forwards to this crate's `embedded-models` feature. Both model files are compiled into
the binary and loaded directly from static memory by rten; they are never extracted to disk,
and no network request is made at runtime. The full model attribution and licences are also
embedded and displayed in About. A complete `PDFCRAFT_MODELS` directory remains an explicit
override; malformed models there return a loading error. An absent/incomplete override falls
back to the embedded models. This adds about 12 MB of model data to the executable.

Embedding is optional so builds of a clean checkout, browser builds and existing installers
do not require model downloads or duplicate their existing external model files. The models
remain ignored build inputs and must not be committed. Standard release packaging continues
to ship external models; the feature is for distributions that want OCR in a single binary.

`cargo test -p pdfcraft-ocr --features embedded-models` verifies recognition through the
in-memory path, attribution, absence of extracted files, and errors for corrupt overrides.
