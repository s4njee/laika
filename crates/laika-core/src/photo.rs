//! Photo model and sync state. SQLite persistence lands in Phase 2;
//! these types fix the vocabulary now so Phase 1 UI compiles against them.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SyncState {
    #[default]
    Local,
    Pending,
    Synced,
    Failed,
}

#[derive(Clone, Debug)]
pub struct Photo {
    pub id: i64,
    pub filename: String,
    pub rating: u8,
    pub picked: bool,
    pub rejected: bool,
    /// V13: color label, 0 = none.
    pub label: u8,
    pub sync: SyncState,
    /// Gradient placeholder seed until real thumbnails land (Phase 2).
    pub tint: (u32, u32),
}

impl Photo {
    pub fn fixture(id: i64, filename: &str, tint: (u32, u32)) -> Self {
        Self {
            id,
            filename: filename.into(),
            rating: 0,
            picked: false,
            rejected: false,
            label: 0,
            sync: SyncState::Local,
            tint,
        }
    }
}
