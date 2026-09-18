use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Write;
use std::path::Path;

/// Represents the local agent state for persistence across restarts.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AgentState {
    pub tenant: String,
    pub last_lease: Option<u32>,
    pub last_bytes: Option<u64>,
}

impl AgentState {
    /// Loads the agent state from the specified checkpoint file.
    pub fn load(path: &Path) -> std::io::Result<Self> {
        let data = std::fs::read_to_string(path)?;
        serde_json::from_str(&data).map_err(std::io::Error::other)
    }

    /// Saves the agent state atomically to the specified checkpoint file.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let temp_path = path.with_extension("tmp");
        let data = serde_json::to_string(self).map_err(std::io::Error::other)?;

        let mut file = File::create(&temp_path)?;
        file.write_all(data.as_bytes())?;
        file.sync_all()?;

        std::fs::rename(temp_path, path)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;

    #[test]
    fn test_agent_state_persistence() {
        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join(format!("agent_state_{}.json", std::process::id()));

        let state = AgentState {
            tenant: "dcc-test".to_string(),
            last_lease: Some(123),
            last_bytes: Some(4096),
        };

        state.save(&path).expect("save");
        let loaded = AgentState::load(&path).expect("load");

        assert_eq!(state, loaded);
        std::fs::remove_file(path).expect("cleanup");
    }
}
