//! Framed, checksummed edit records.

use editor_core::{Edit, EditBatch};

const CHECKSUM_LEN: usize = 16;

fn checksum(payload: &[u8]) -> [u8; CHECKSUM_LEN] {
    let mut out = [0u8; CHECKSUM_LEN];
    out.copy_from_slice(&blake3::hash(payload).as_bytes()[..CHECKSUM_LEN]);
    out
}

/// Appends one framed record for `batch` to `out`.
pub(super) fn encode_record(batch: &EditBatch, out: &mut Vec<u8>) {
    let mut payload = Vec::new();
    payload.extend_from_slice(&(batch.0.len() as u32).to_le_bytes());
    for edit in &batch.0 {
        payload.extend_from_slice(&(edit.range.start as u64).to_le_bytes());
        payload.extend_from_slice(&(edit.range.end as u64).to_le_bytes());
        payload.extend_from_slice(&(edit.text.len() as u32).to_le_bytes());
        payload.extend_from_slice(edit.text.as_bytes());
    }
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&checksum(&payload));
    out.extend_from_slice(&payload);
}

fn take<'a>(bytes: &mut &'a [u8], len: usize) -> Option<&'a [u8]> {
    if bytes.len() < len {
        return None;
    }
    let (head, tail) = bytes.split_at(len);
    *bytes = tail;
    Some(head)
}

fn take_u32(bytes: &mut &[u8]) -> Option<u32> {
    Some(u32::from_le_bytes(take(bytes, 4)?.try_into().ok()?))
}

fn take_u64(bytes: &mut &[u8]) -> Option<u64> {
    Some(u64::from_le_bytes(take(bytes, 8)?.try_into().ok()?))
}

fn decode_payload(mut payload: &[u8]) -> Option<EditBatch> {
    let count = take_u32(&mut payload)?;
    let mut edits = Vec::new();
    for _ in 0..count {
        let start = take_u64(&mut payload)? as usize;
        let end = take_u64(&mut payload)? as usize;
        let len = take_u32(&mut payload)? as usize;
        let text = std::str::from_utf8(take(&mut payload, len)?)
            .ok()?
            .to_string();
        edits.push(Edit {
            range: start..end,
            text,
        });
    }
    payload.is_empty().then_some(EditBatch(edits))
}

/// Reads records until the data ends or stops being valid.
/// The flag is true when damaged data was found.
pub(super) fn decode_records(mut bytes: &[u8]) -> (Vec<EditBatch>, bool) {
    let mut batches = Vec::new();
    while !bytes.is_empty() {
        let record = (|| {
            let len = take_u32(&mut bytes)? as usize;
            let expected = take(&mut bytes, CHECKSUM_LEN)?;
            let payload = take(&mut bytes, len)?;
            if checksum(payload) != expected {
                return None;
            }
            decode_payload(payload)
        })();
        match record {
            Some(batch) => batches.push(batch),
            None => return (batches, true),
        }
    }
    (batches, false)
}
