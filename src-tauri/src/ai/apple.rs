use fm_bridge::Bridge;

pub struct AppleAiProvider {
    pub bridge: Bridge,
}

impl AppleAiProvider {
    pub fn new() -> Result<Self, String> {
        // Reads FM_BRIDGE_BIN from the environment
        let bridge = Bridge::from_env().map_err(|e| e.to_string())?;
        Ok(Self { bridge })
    }
}
