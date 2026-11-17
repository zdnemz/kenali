# Models

Kenali uses four ONNX weights plus its own ONNX Runtime DLL. **Weights are not
committed.** This directory contains only the committed manifest
[`models.lock`](models.lock) and this README; every `*.onnx` file (and
`onnxruntime.dll`) is fetched at build/install time and is gitignored
(`*.onnx`, `models/*.onnx` in the repository `.gitignore`).

## Fetching and verification

Weights are fetched by developer tooling, `cargo xtask models fetch|verify`
(a later wave), into `models/`. Fetch downloads each asset listed in
`models.lock`; verify recomputes its SHA-256 and checks it against the
`sha256` column of the matching row. **A model's SHA-256 must be verified
against `models.lock` before any ONNX session is created** (context pack §2
item 10 and §3.4). No session is opened on an unverified file.

The `sha256` and `upstream_commit` cells in `models.lock` are currently
`VERIFY:<...>` placeholders: the real values could not be confirmed on the
Linux authoring host because no weights were present. They must be filled
from upstream by the owner (see `docs/reports/models.md`) before `verify`
can actually succeed.

## Loading: absolute paths only

DLLs and models are **loaded by absolute path** from

```
C:\Program Files\Kenali\<version>\models\
```

A `KENALI_MODEL_DIR` environment variable is the optional development/test
override for that absolute directory. Relative paths are never used: the
service resolves the directory to an absolute path and passes it to
`ort::init_from(<absolute onnxruntime.dll>)` and to each model load. This
prevents a relative-path/CWD trick from swapping in a malicious file.

## onnxruntime.dll must be Kenali's own pinned copy

`ort =2.0.0-rc.13` is loaded with its `load-dynamic` feature, and Kenali
supplies **its own pinned `onnxruntime.dll`** (recorded in `models.lock`,
load-dynamic by absolute path). Do not rely on the system copy: Windows ships
an older `onnxruntime.dll`, and loading the wrong one produces `0xc000007b`
(bad-image / mixed 32- and 64-bit) failures and is a DLL-hijack risk. The
pinned DLL is fetched and hash-verified exactly like the ONNX weights.

## Licences and exclusions

- YuNet — MIT; SFace — Apache-2.0; MiniFASNet V2 + V1SE — Apache-2.0;
  ONNX Runtime — MIT.
- InsightFace pretrained weights such as `buffalo_l` are **non-commercial**
  (context pack §3.5, R25) and are **excluded** — do not bundle or download
  them by default. A separate licence would be required.

## Host status

**No model was downloaded, hashed or executed on this host.** This Linux
authoring environment has no weights and no Windows runtime; all fetch,
hash verification and ONNX inference is owner-run in the Windows VM.
