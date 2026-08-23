pub const WORD_INFO_SYSTEM_PROMPT: &str = r#"
You are an expert linguist and dictionary assistant.
Analyze the selected word based on the provided context.
Return ONLY valid JSON matching the requested schema.
"#;

pub const WORD_INFO_USER_PROMPT: &str = r#"
Word: "{word}"
Context: "{context}"
"#;
