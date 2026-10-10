//! Durable storage of the JMAP `Email/changes` state cursor.
//!
//! tinyclaw treats the mailbox as its durable queue, so the state string must
//! survive a restart. The trait keeps the storage pluggable: the process can
//! back it with a file, a key-value store or a test fixture.
use crate::types::ChangesState;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Persists the opaque JMAP state string per account.
pub trait StateCursorStore: Send + Sync + std::fmt::Debug {
    /// Loads the cursor for an account, or `None` when none was stored yet.
    fn load(&self, account_id: &str) -> Result<Option<String>>;

    /// Stores the cursor for an account, replacing any previous value.
    fn store(&self, account_id: &str, state: &str) -> Result<()>;

    /// Removes the cursor for an account.
    fn clear(&self, account_id: &str) -> Result<()>;

    /// Convenience wrapper that returns the whole state record.
    fn load_state(&self, account_id: &str) -> Result<Option<ChangesState>> {
        Ok(self
            .load(account_id)?
            .map(|state| ChangesState::new(account_id, state)))
    }
}

/// A cursor store held in memory, useful for tests and single-shot runs.
#[derive(Debug, Default)]
pub struct MemoryCursorStore {
    states: Mutex<std::collections::HashMap<String, String>>,
}

impl MemoryCursorStore {
    /// Creates an empty in-memory store.
    pub fn new() -> Self {
        Self::default()
    }
}

impl StateCursorStore for MemoryCursorStore {
    fn load(&self, account_id: &str) -> Result<Option<String>> {
        let states = self
            .states
            .lock()
            .map_err(|_| anyhow::anyhow!("cursor store lock poisoned"))?;
        Ok(states.get(account_id).cloned())
    }

    fn store(&self, account_id: &str, state: &str) -> Result<()> {
        let mut states = self
            .states
            .lock()
            .map_err(|_| anyhow::anyhow!("cursor store lock poisoned"))?;
        states.insert(account_id.to_string(), state.to_string());
        Ok(())
    }

    fn clear(&self, account_id: &str) -> Result<()> {
        let mut states = self
            .states
            .lock()
            .map_err(|_| anyhow::anyhow!("cursor store lock poisoned"))?;
        states.remove(account_id);
        Ok(())
    }
}

/// A cursor store backed by one JSON file, written atomically.
///
/// The file holds a map of account ID to state string. Writes go to a temporary
/// file in the same directory and are then renamed, so a crash mid-write leaves
/// the previous cursor intact.
#[derive(Debug)]
pub struct FileCursorStore {
    path: PathBuf,
    states: Mutex<std::collections::HashMap<String, String>>,
}

impl FileCursorStore {
    /// Opens or creates a cursor file, loading any existing states.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let states = if path.exists() {
            let contents = std::fs::read_to_string(&path)
                .with_context(|| format!("Failed to read cursor file {}", path.display()))?;
            if contents.trim().is_empty() {
                std::collections::HashMap::new()
            } else {
                serde_json::from_str(&contents)
                    .with_context(|| format!("Failed to parse cursor file {}", path.display()))?
            }
        } else {
            std::collections::HashMap::new()
        };
        Ok(Self {
            path,
            states: Mutex::new(states),
        })
    }

    /// Persists the in-memory map to disk atomically.
    fn flush(&self, states: &std::collections::HashMap<String, String>) -> Result<()> {
        if let Some(parent) = self.path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create cursor directory {}", parent.display())
            })?;
        }
        let serialised = serde_json::to_vec_pretty(states)?;
        let temp_path = self.path.with_extension("tmp");
        std::fs::write(&temp_path, &serialised)
            .with_context(|| format!("Failed to write cursor file {}", temp_path.display()))?;
        std::fs::rename(&temp_path, &self.path)
            .with_context(|| format!("Failed to replace cursor file {}", self.path.display()))?;
        Ok(())
    }
}

impl StateCursorStore for FileCursorStore {
    fn load(&self, account_id: &str) -> Result<Option<String>> {
        let states = self
            .states
            .lock()
            .map_err(|_| anyhow::anyhow!("cursor store lock poisoned"))?;
        Ok(states.get(account_id).cloned())
    }

    fn store(&self, account_id: &str, state: &str) -> Result<()> {
        let mut states = self
            .states
            .lock()
            .map_err(|_| anyhow::anyhow!("cursor store lock poisoned"))?;
        states.insert(account_id.to_string(), state.to_string());
        self.flush(&states)
    }

    fn clear(&self, account_id: &str) -> Result<()> {
        let mut states = self
            .states
            .lock()
            .map_err(|_| anyhow::anyhow!("cursor store lock poisoned"))?;
        states.remove(account_id);
        self.flush(&states)
    }
}
