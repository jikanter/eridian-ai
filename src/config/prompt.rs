use super::*;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Prompt {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub content: String
}

impl Prompt {
    pub fn new(name: &str, content: &str) -> Self {
        Self {
            name: name.to_string(),
            content: content.to_string()
        }
    }
}

