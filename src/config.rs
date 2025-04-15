use serde::Deserialize;
use std::fs;
use tracing::Level;

#[derive(Deserialize, Debug, Clone)]
pub struct Config {
    pub pem_file: String,
    pub ic_url: String,
    pub oc_public_key: String,
    pub port: u16,
    #[serde(with = "LevelDef")]
    pub log_level: Level,
    pub mistral_api_key: Option<String>,
    pub sqlite_db_path: Option<String>,
    pub agent: AgentConfig,
}

#[derive(Deserialize, Debug, Clone)]
pub struct AgentConfig {
    pub enable_agent_planning: bool,
    pub enable_memory: bool,
    pub enable_summarization: bool,
    pub enable_moderation: bool,
    pub memory_retention_days: u32,
    pub max_memory_items: usize,
}

#[derive(Deserialize)]
#[serde(remote = "Level")]
enum LevelDef {
    TRACE,
    DEBUG,
    INFO,
    WARN,
    ERROR,
}

impl Config {
    pub fn from_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }
    
    pub fn mistral_api_key(&self) -> Result<String, String> {
        if let Some(key) = &self.mistral_api_key {
            if !key.is_empty() {
                return Ok(key.clone());
            }
        }
        
        // Try to get from environment
        if let Ok(key) = std::env::var("MISTRAL_API_KEY") {
            if !key.is_empty() {
                return Ok(key);
            }
        }
        
        Err("Mistral API key not found in config or environment".to_string())
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            enable_agent_planning: true,
            enable_memory: true,
            enable_summarization: false,
            enable_moderation: false,
            memory_retention_days: 30,
            max_memory_items: 1000,
        }
    }
} 