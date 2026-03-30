struct HeadWindowEntry {
    message_id: uuid::Uuid,
    segment_index: u64,
    file_offset: u64,
}

struct HeadWindow {
    entries: Vec<HeadWindowEntry>,
}