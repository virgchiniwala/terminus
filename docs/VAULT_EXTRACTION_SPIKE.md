# Vault Extraction Spike (Phase 0)
Last updated: 2026-02-26

## Purpose
De-risk Terminus's new document-workflow wedge before building Vault ingestion and `ReadVaultFile`.

This spike validates two things:
1. **Crate viability**: can Rust dependencies parse/open the target file types without dependency conflicts?
2. **Extraction fidelity**: is the extracted output structurally useful for professional document review workflows?

## Scope (Phase 0 only)
- PDF extraction (viability + fidelity)
- DOCX extraction (dependency viability wired; parser fidelity validation still manual)
- XLSX extraction (viability + fidelity)
- Tauri v2 dialog plugin wiring (`tauri-plugin-dialog`)
- Cargo dependency compatibility (`cargo check`)

## What shipped in this spike
- New backend spike module: `/Users/vir.c/terminus/src-tauri/src/vault_spike.rs`
- New Tauri command: `probe_vault_extraction`
- New dependencies added to `/Users/vir.c/terminus/src-tauri/Cargo.toml`:
  - `pdf-extract`
  - `docx-rs`
  - `calamine`
  - `tauri-plugin-dialog`
- Tauri plugin initialized in `/Users/vir.c/terminus/src-tauri/src/main.rs`
- Tauri v2 capability updated:
  - `/Users/vir.c/terminus/src-tauri/capabilities/main-window.json`
  - added `dialog:allow-open`

## Current spike command (dev use)
`probe_vault_extraction({ path, maxPreviewChars? })`

Returns:
- detected file type
- file size
- extracted char count
- bounded preview excerpt (default 3,000 chars; hard cap 8,000)
- extraction status / notes

## Important current limitation (intentional for spike)
`DOCX` parsing is **not fully implemented yet** in this spike.

Reason:
- `docx-rs` dependency is now wired and compiles (crate viability path)
- Parser API/fidelity still needs direct validation on real professional `.docx` examples before we commit the Vault implementation shape

This is acceptable for Phase 0 because the goal is to de-risk dependencies first and record fidelity results before building `ReadVaultFile`.

## Manual validation checklist (run on real files)

### 1) PDF (digital docs, not scans)
- Use a real contract / NDA with clause numbering
- Use a deal deck / CIM excerpt with tables and financial numbers
- Validate:
  - clause numbering survives
  - key figures are present and readable
  - output is not garbled enough to break review workflow

### 2) DOCX
- Use a Word doc with headings, bullets, and tables
- Validate:
  - headings preserved
  - bullets readable
  - table cell text extractable
- If parser fidelity is weak, record limitation before Phase 1

### 3) XLSX
- Use a workbook with multiple sheets and formulas
- Validate:
  - rows/cells readable as tabular text
  - computed values are usable for review (not only formulas)

### 4) Tauri dialog plugin
- Confirm file-open dialog works in macOS app
- Confirm selected path returned is absolute and matches file type restrictions (when UI is added)

## Suggested test files (minimum)
- 1x PDF contract / legal doc
- 1x PDF deck or data-room style doc
- 1x DOCX standard/template doc
- 1x XLSX workbook export/model

## Results log (fill during spike)
| File | Type | Viability | Fidelity | Notes |
|---|---|---|---|---|
| (pending) | PDF | | | |
| (pending) | DOCX | | | |
| (pending) | XLSX | | | |

## Go / No-Go Gate
Proceed to Vault Phase 1 only if:
- crate dependencies compile cleanly with Terminus
- extraction output is structurally useful for "Document Review Against Standard"
- known limitations are explicitly documented (e.g., scanned PDFs)

If any core format fails viability or fidelity, stop and assess alternatives before building Vault.
