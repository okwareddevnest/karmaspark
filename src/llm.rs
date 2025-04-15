use anyhow::{anyhow, Result};
use async_openai::{
    config::OpenAIConfig,
    types::{
        ChatCompletionRequestMessage, CreateChatCompletionRequest, Role,
    },
    Client,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::sleep;
use tracing::{error, info, warn};

use crate::memory::EmbeddingModel;

const MISTRAL_API_URL: &str = "https://api.mistral.ai/v1";
const MAX_RETRIES: usize = 3;
const RETRY_DELAY_MS: u64 = 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct MistralClient {
    client: Client<OpenAIConfig>,
    model: String,
}

impl MistralClient {
    pub fn new(api_key: &str) -> Self {
        let config = OpenAIConfig::new()
            .with_api_key(api_key)
            .with_api_base(MISTRAL_API_URL);
        
        let client = Client::with_config(config);
        
        Self {
            client,
            model: "mistral-medium".to_string(), // Default model
        }
    }
    
    pub fn with_model(mut self, model: &str) -> Self {
        self.model = model.to_string();
        self
    }
    
    pub async fn chat(
        &self,
        system_prompt: &str,
        messages: &[ChatMessage],
    ) -> Result<String> {
        // Convert messages to OpenAI format
        let mut chat_messages: Vec<ChatCompletionRequestMessage> = Vec::new();
        
        // Add system message
        chat_messages.push(ChatCompletionRequestMessage {
            content: Some(system_prompt.to_string()),
            name: None,
            role: Role::System,
            function_call: None,
        });
        
        // Add user/assistant messages
        for msg in messages {
            match msg.role.as_str() {
                "user" => {
                    chat_messages.push(ChatCompletionRequestMessage {
                        content: Some(msg.content.clone()),
                        name: None,
                        role: Role::User,
                        function_call: None,
                    });
                }
                "assistant" => {
                    chat_messages.push(ChatCompletionRequestMessage {
                        content: Some(msg.content.clone()),
                        name: None,
                        role: Role::Assistant,
                        function_call: None,
                    });
                }
                _ => {
                    return Err(anyhow!("Unsupported message role: {}", msg.role));
                }
            }
        }
        
        // Create request
        let request = CreateChatCompletionRequest {
            model: self.model.clone(),
            messages: chat_messages,
            temperature: Some(0.7),
            top_p: Some(0.95),
            max_tokens: Some(1024),
            stream: Some(false),
            frequency_penalty: Some(0.0),
            presence_penalty: Some(0.0),
            ..Default::default()
        };
        
        // Send request with retry logic
        let mut retries = 0;
        let mut last_error = None;
        
        while retries < MAX_RETRIES {
            match self.client.chat().create(request.clone()).await {
                Ok(response) => {
                    // Extract response
                    let choice = response
                        .choices
                        .first()
                        .ok_or_else(|| anyhow!("No choices in response"))?;
                    
                    let content = choice
                        .message
                        .content
                        .clone()
                        .unwrap_or_default();
                    
                    return Ok(content);
                },
                Err(e) => {
                    let error_string = e.to_string();
                    
                    // Check for rate limit errors
                    if error_string.contains("rate limit") || error_string.contains("Requests rate limit exceeded") {
                        retries += 1;
                        if retries < MAX_RETRIES {
                            let backoff = RETRY_DELAY_MS * (2_u64.pow(retries as u32));
                            info!("Rate limit exceeded, retrying in {}ms (attempt {}/{})", 
                                  backoff, retries, MAX_RETRIES);
                            sleep(Duration::from_millis(backoff)).await;
                            continue;
                        } else {
                            error!("Rate limit exceeded after {} retries: {}", retries, error_string);
                            return Err(anyhow!("Rate limit exceeded. Please try again in a few minutes."));
                        }
                    }
                    
                    // For other errors
                    error!("Error from Mistral API: {}", error_string);
                    last_error = Some(e);
                    break;
                }
            }
        }
        
        // If we got here, all retries failed
        Err(anyhow!("API error after {} retries: {}", 
                    retries, 
                    last_error.map_or("Unknown error".to_string(), |e| e.to_string())))
    }
    
    pub async fn summarize(&self, text: &str) -> Result<String> {
        let system_prompt = "You are a highly efficient text summarizer. Create a concise summary of the following text while retaining the key points.";
        
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: text.to_string(),
        }];
        
        self.chat(system_prompt, &messages).await
    }
    
    pub async fn moderate(&self, text: &str) -> Result<(bool, String)> {
        let system_prompt = "You are a content moderation system. Analyze the following text for any harmful, offensive, or inappropriate content. If you find such content, respond with 'FLAGGED: <reason>'. If the content is safe, respond with 'SAFE'.";
        
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: text.to_string(),
        }];
        
        let response = self.chat(system_prompt, &messages).await?;
        
        let is_flagged = response.starts_with("FLAGGED:");
        Ok((is_flagged, response))
    }
}

// Implementation of embedding model using Mistral API
pub struct MistralEmbedding {
    client: Client<OpenAIConfig>,
    model: String,
}

impl MistralEmbedding {
    pub fn new(api_key: &str) -> Self {
        let config = OpenAIConfig::new()
            .with_api_key(api_key)
            .with_api_base(MISTRAL_API_URL);
        
        let client = Client::with_config(config);
        
        Self {
            client,
            model: "mistral-embed".to_string(),
        }
    }
}

#[async_trait]
impl EmbeddingModel for MistralEmbedding {
    async fn embed_text(&self, text: &str) -> Result<Vec<f32>> {
        let request = async_openai::types::CreateEmbeddingRequest {
            model: self.model.clone(),
            input: async_openai::types::EmbeddingInput::String(text.to_string()),
            user: None,
        };
        
        // Send request with retry logic
        let mut retries = 0;
        let mut last_error = None;
        
        while retries < MAX_RETRIES {
            match self.client.embeddings().create(request.clone()).await {
                Ok(response) => {
                    let embedding = response
                        .data
                        .first()
                        .ok_or_else(|| anyhow!("No embedding returned"))?
                        .embedding
                        .clone();
                    
                    return Ok(embedding);
                },
                Err(e) => {
                    let error_string = e.to_string();
                    
                    // Check for rate limit errors
                    if error_string.contains("rate limit") || error_string.contains("Requests rate limit exceeded") {
                        retries += 1;
                        if retries < MAX_RETRIES {
                            let backoff = RETRY_DELAY_MS * (2_u64.pow(retries as u32));
                            info!("Rate limit exceeded for embeddings, retrying in {}ms (attempt {}/{})", 
                                  backoff, retries, MAX_RETRIES);
                            sleep(Duration::from_millis(backoff)).await;
                            continue;
                        } else {
                            error!("Rate limit exceeded for embeddings after {} retries: {}", retries, error_string);
                            return Err(anyhow!("Rate limit exceeded. Please try again in a few minutes."));
                        }
                    }
                    
                    // For other errors
                    error!("Error from Mistral API when creating embeddings: {}", error_string);
                    last_error = Some(e);
                    break;
                }
            }
        }
        
        // If we got here, all retries failed
        Err(anyhow!("API error after {} retries: {}", 
                    retries, 
                    last_error.map_or("Unknown error".to_string(), |e| e.to_string())))
    }
    
    async fn similarity(&self, embedding1: &[f32], embedding2: &[f32]) -> f32 {
        // Cosine similarity calculation
        if embedding1.len() != embedding2.len() || embedding1.is_empty() {
            return 0.0;
        }
        
        let dot_product: f32 = embedding1.iter().zip(embedding2.iter()).map(|(x, y)| x * y).sum();
        let magnitude1: f32 = embedding1.iter().map(|x| x * x).sum::<f32>().sqrt();
        let magnitude2: f32 = embedding2.iter().map(|x| x * x).sum::<f32>().sqrt();
        
        if magnitude1 == 0.0 || magnitude2 == 0.0 {
            return 0.0;
        }
        
        dot_product / (magnitude1 * magnitude2)
    }
} 