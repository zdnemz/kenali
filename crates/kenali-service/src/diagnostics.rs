//! Redacted diagnostics. The admin interface accepts **no file paths**: a
//! caller sends `ExportDiagnostics`, the service builds a generic report from
//! in-memory state, redacts it, and streams it back solely as
//! [`AdminResponse::DiagnosticsChunk`] bytes (≤64 KB per chunk, ≤10 MB total).
//! No embedding, secret, SID, path or score is ever included.

use kenali_core::config::ConfigReport;
use kenali_core::types::StatusReport;
use kenali_ipc::admin::AdminResponse;
use kenali_ipc::codec::{MAX_DIAG_CHUNK_BYTES, MAX_DIAG_TOTAL_BYTES};
use kenali_ipc::wire::Bytes;

/// One chunk of the redacted report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsChunk {
    pub index: u32,
    pub total: u32,
    pub bytes: Vec<u8>,
}

/// Replace any token that could carry an identifier, path, decimal score or
/// binary blob with a fixed marker. Deliberately conservative: when in doubt,
/// redact.
pub fn redact(text: &str) -> String {
    text.split_whitespace()
        .map(|token| if is_sensitive(token) { "[redacted]" } else { token })
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_sensitive(token: &str) -> bool {
    if token.starts_with("S-1-") || token.contains('\\') || token.contains('@') {
        return true;
    }
    if token.parse::<f32>().is_ok() {
        return true;
    }
    let hex_len = token.bytes().filter(|b| b.is_ascii_hexdigit()).count();
    hex_len >= 8 && hex_len == token.len()
}

/// Build a generic, score-free report from in-memory status/config. The final
/// text is passed through [`redact`] as a backstop.
pub fn build_report(status: &StatusReport, config: &ConfigReport) -> Vec<u8> {
    let enrolled = if status.enrolled_sid.is_some() { "yes" } else { "no" };
    let pause = if status.pause_note.is_some() { "paused" } else { "not-paused" };
    let config_entries = config.entries.len();
    let raw = format!(
        "kenali diagnostics v1\n\
         enabled={} camera_ok={} kill_switch={} face_paused={} {}\n\
         enrolled={} service_pid={} template_count={}\n\
         config_entries={}\n",
        status.enabled,
        status.camera_ok,
        status.kill_switch,
        status.face_paused,
        pause,
        enrolled,
        status.service_pid,
        status.template_count,
        config_entries,
    );
    redact(&raw).into_bytes()
}

/// Split a report into bounded chunks, truncating at the total cap. Never
/// produces a chunk larger than the codec's per-chunk cap.
pub fn chunk(bytes: &[u8]) -> Vec<DiagnosticsChunk> {
    let capped = if bytes.len() > MAX_DIAG_TOTAL_BYTES {
        &bytes[..MAX_DIAG_TOTAL_BYTES]
    } else {
        bytes
    };
    if capped.is_empty() {
        return Vec::new();
    }
    let chunk_count = capped.len().div_ceil(MAX_DIAG_CHUNK_BYTES);
    let total = u32::try_from(chunk_count).unwrap_or(u32::MAX);
    let mut out = Vec::with_capacity(chunk_count);
    for (index, piece) in capped.chunks(MAX_DIAG_CHUNK_BYTES).enumerate() {
        out.push(DiagnosticsChunk {
            index: index as u32,
            total,
            bytes: piece.to_vec(),
        });
    }
    out
}

/// Wrap chunks as typed admin responses.
pub fn into_admin_responses(chunks: Vec<DiagnosticsChunk>) -> Vec<AdminResponse> {
    chunks
        .into_iter()
        .map(|chunk| AdminResponse::DiagnosticsChunk {
            index: chunk.index,
            total: chunk.total,
            bytes: Bytes::new(chunk.bytes),
        })
        .collect()
}

/// Convenience: report → bounded typed responses.
pub fn export(status: &StatusReport, config: &ConfigReport) -> Vec<AdminResponse> {
    let report = build_report(status, config);
    into_admin_responses(chunk(&report))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kenali_core::config::ConfigValue;
    use kenali_core::config::ConfigKey;
    use kenali_core::types::Sid;

    fn status() -> StatusReport {
        StatusReport {
            enabled: true,
            camera_ok: true,
            enrolled_sid: Some(Sid::new("S-1-5-21-secret")),
            kill_switch: false,
            face_paused: false,
            pause_note: Some("Face unlock paused".to_string()),
            service_pid: 4242,
            template_count: 2,
        }
    }

    #[test]
    fn report_never_contains_sid_or_decimal_score() {
        let report = build_report(&status(), &ConfigReport::default_policy());
        let text = String::from_utf8_lossy(&report);
        assert!(!text.contains("S-1-5-21-secret"));
        assert!(!text.contains("0.363"));
        assert!(!text.contains("secret"));
    }

    #[test]
    fn redact_removes_paths_sids_and_decimals() {
        let redacted = redact("sid S-1-5-21-1 path C:\\secret score 0.363 ok good");
        assert!(!redacted.contains("S-1-5-21-1"));
        assert!(!redacted.contains("C:\\secret"));
        assert!(!redacted.contains("0.363"));
        assert!(redacted.contains("ok good"));
    }

    #[test]
    fn chunking_respects_the_per_chunk_cap() {
        let bytes = vec![7u8; MAX_DIAG_CHUNK_BYTES + 10];
        let chunks = chunk(&bytes);
        assert_eq!(chunks.len(), 2);
        assert!(chunks.iter().all(|c| c.bytes.len() <= MAX_DIAG_CHUNK_BYTES));
        assert_eq!(chunks[0].index, 0);
        assert_eq!(chunks[1].index, 1);
        assert!(chunks.iter().all(|c| c.total == 2));
    }

    #[test]
    fn chunking_caps_total_bytes() {
        let bytes = vec![1u8; MAX_DIAG_TOTAL_BYTES + 100];
        let chunks = chunk(&bytes);
        let total: usize = chunks.iter().map(|c| c.bytes.len()).sum();
        assert_eq!(total, MAX_DIAG_TOTAL_BYTES);
    }

    #[test]
    fn empty_report_yields_no_chunks() {
        assert!(chunk(&[]).is_empty());
        assert!(export(&status(), &ConfigReport::default_policy()).len() > 0);
        let mut cfg = ConfigReport::default_policy();
        let _ = cfg.set(ConfigKey::PadEnabled, ConfigValue::Bool(true));
        assert!(export(&status(), &cfg).len() > 0);
    }
}
