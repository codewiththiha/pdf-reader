pub struct MockAiProvider;

impl MockAiProvider {
    pub fn new() -> Result<Self, String> {
        // Fails gracefully on unsupported platforms
        Err("Apple Intelligence is only available on Apple Silicon Macs.".to_string())
    }
}
