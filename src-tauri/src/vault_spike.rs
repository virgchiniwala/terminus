use calamine::{open_workbook_auto, Reader};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

const DEFAULT_MAX_PREVIEW_CHARS: usize = 3_000;
const ABSOLUTE_MAX_PREVIEW_CHARS: usize = 8_000;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VaultSpikeFileType {
    Pdf,
    Docx,
    Xlsx,
    Markdown,
    Text,
    Unsupported,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultExtractionProbe {
    pub path: String,
    pub file_type: VaultSpikeFileType,
    pub size_bytes: u64,
    pub extracted_chars: usize,
    pub preview_chars: usize,
    pub was_truncated: bool,
    pub preview_excerpt: String,
    pub extraction_status: String,
    pub notes: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum VaultSpikeError {
    #[error("File path is required.")]
    MissingPath,
    #[error("File was not found.")]
    NotFound,
    #[error("Only PDF, DOCX, XLSX, MD, and TXT files are supported in the spike.")]
    UnsupportedFileType,
    #[error("{0}")]
    Extraction(String),
}

pub fn probe_extraction(
    path: &str,
    requested_max_preview_chars: Option<usize>,
) -> Result<VaultExtractionProbe, VaultSpikeError> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(VaultSpikeError::MissingPath);
    }
    let path_buf = PathBuf::from(trimmed);
    if !path_buf.exists() {
        return Err(VaultSpikeError::NotFound);
    }
    let metadata = fs::metadata(&path_buf)
        .map_err(|e| VaultSpikeError::Extraction(format!("Could not read file metadata: {e}")))?;
    let file_type = detect_file_type(&path_buf);
    let max_preview_chars = requested_max_preview_chars
        .unwrap_or(DEFAULT_MAX_PREVIEW_CHARS)
        .clamp(1, ABSOLUTE_MAX_PREVIEW_CHARS);

    let (text, mut notes, extraction_status) = match file_type {
        VaultSpikeFileType::Pdf => (
            extract_pdf_text(&path_buf)?,
            vec![
                "PDF extraction uses pdf-extract. Scan-only PDFs may return little or no text."
                    .to_string(),
            ],
            "ok".to_string(),
        ),
        VaultSpikeFileType::Xlsx => (
            extract_xlsx_text(&path_buf)?,
            vec!["XLSX extraction uses calamine and returns tabular text per sheet.".to_string()],
            "ok".to_string(),
        ),
        VaultSpikeFileType::Markdown | VaultSpikeFileType::Text => (
            fs::read_to_string(&path_buf).map_err(|e| {
                VaultSpikeError::Extraction(format!("Could not read text file: {e}"))
            })?,
            vec![],
            "ok".to_string(),
        ),
        VaultSpikeFileType::Docx => {
            // The spike intentionally proves dependency resolution now and records parser viability next.
            // `docx-rs` is linked in Cargo.toml for the spike gate.
            let _touch_dependency = docx_rs::Docx::new();
            (
                String::new(),
                vec![
                    "DOCX parser fidelity still needs manual validation in this spike.".to_string(),
                    "Dependency resolution is wired (`docx-rs` present), but parser implementation is intentionally deferred pending crate API confirmation.".to_string(),
                ],
                "needs_manual_validation".to_string(),
            )
        }
        VaultSpikeFileType::Unsupported => return Err(VaultSpikeError::UnsupportedFileType),
    };

    let normalized = normalize_extracted_text(&text);
    let extracted_chars = normalized.chars().count();
    let (preview_excerpt, preview_chars, was_truncated) =
        truncate_chars(&normalized, max_preview_chars);
    if extracted_chars == 0 {
        notes.push("No extractable text was found. This may be a scanned PDF/image-only file or an unsupported document structure.".to_string());
    }

    Ok(VaultExtractionProbe {
        path: trimmed.to_string(),
        file_type,
        size_bytes: metadata.len(),
        extracted_chars,
        preview_chars,
        was_truncated,
        preview_excerpt,
        extraction_status,
        notes,
    })
}

fn detect_file_type(path: &Path) -> VaultSpikeFileType {
    let ext = path
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "pdf" => VaultSpikeFileType::Pdf,
        "docx" => VaultSpikeFileType::Docx,
        "xlsx" | "xlsm" | "xls" => VaultSpikeFileType::Xlsx,
        "md" | "markdown" => VaultSpikeFileType::Markdown,
        "txt" => VaultSpikeFileType::Text,
        _ => VaultSpikeFileType::Unsupported,
    }
}

fn extract_pdf_text(path: &Path) -> Result<String, VaultSpikeError> {
    pdf_extract::extract_text(path)
        .map_err(|e| VaultSpikeError::Extraction(format!("PDF extraction failed: {e}")))
}

fn extract_xlsx_text(path: &Path) -> Result<String, VaultSpikeError> {
    let mut workbook = open_workbook_auto(path)
        .map_err(|e| VaultSpikeError::Extraction(format!("XLSX open failed: {e}")))?;
    let mut out = String::new();
    let sheet_names = workbook.sheet_names().to_owned();
    for sheet_name in sheet_names {
        out.push_str("## Sheet: ");
        out.push_str(&sheet_name);
        out.push('\n');
        match workbook.worksheet_range(&sheet_name) {
            Ok(range) => {
                for row in range.rows() {
                    let line = row
                        .iter()
                        .map(|cell| cell.to_string())
                        .collect::<Vec<String>>()
                        .join(" | ");
                    if !line.trim().is_empty() {
                        out.push_str(&line);
                        out.push('\n');
                    }
                }
            }
            Err(err) => {
                out.push_str("[sheet_read_error] ");
                out.push_str(&err.to_string());
                out.push('\n');
            }
        }
        out.push('\n');
    }
    Ok(out)
}

fn normalize_extracted_text(input: &str) -> String {
    input
        .replace('\u{0000}', "")
        .lines()
        .map(str::trim_end)
        .collect::<Vec<&str>>()
        .join("\n")
}

fn truncate_chars(input: &str, max_chars: usize) -> (String, usize, bool) {
    if input.chars().count() <= max_chars {
        return (input.to_string(), input.chars().count(), false);
    }
    let mut out = String::new();
    let mut count = 0usize;
    for ch in input.chars() {
        if count >= max_chars {
            break;
        }
        out.push(ch);
        count += 1;
    }
    (out, count, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_respects_absolute_cap_and_marks_truncated() {
        let text = "a".repeat(10_000);
        let result = probe_extraction_result_for_test(text, Some(20_000));
        assert_eq!(result.preview_chars, ABSOLUTE_MAX_PREVIEW_CHARS);
        assert!(result.was_truncated);
    }

    #[test]
    fn file_type_detection_matches_supported_extensions() {
        assert!(matches!(
            detect_file_type(Path::new("/tmp/example.pdf")),
            VaultSpikeFileType::Pdf
        ));
        assert!(matches!(
            detect_file_type(Path::new("/tmp/example.docx")),
            VaultSpikeFileType::Docx
        ));
        assert!(matches!(
            detect_file_type(Path::new("/tmp/example.xlsx")),
            VaultSpikeFileType::Xlsx
        ));
    }

    fn probe_extraction_result_for_test(
        text: String,
        requested_max_preview_chars: Option<usize>,
    ) -> VaultExtractionProbe {
        let max_preview_chars = requested_max_preview_chars
            .unwrap_or(DEFAULT_MAX_PREVIEW_CHARS)
            .clamp(1, ABSOLUTE_MAX_PREVIEW_CHARS);
        let normalized = normalize_extracted_text(&text);
        let (preview_excerpt, preview_chars, was_truncated) =
            truncate_chars(&normalized, max_preview_chars);
        VaultExtractionProbe {
            path: "/tmp/test.txt".to_string(),
            file_type: VaultSpikeFileType::Text,
            size_bytes: normalized.len() as u64,
            extracted_chars: normalized.chars().count(),
            preview_chars,
            was_truncated,
            preview_excerpt,
            extraction_status: "ok".to_string(),
            notes: vec![],
        }
    }
}
