use async_trait::async_trait;
use oc_bots_sdk::api::command::{CommandHandler, SuccessResult};
use oc_bots_sdk::api::definition::*;
use oc_bots_sdk::types::{BotCommandContext, BotCommandScope};
use oc_bots_sdk_offchain::AgentRuntime;
use oc_bots_sdk::oc_api::client::Client;
use std::sync::LazyLock;
use std::sync::Arc;
use chrono::Utc;
use tracing::{error, info};

use crate::memory::{Memory, MemoryStore, EmbeddingModel};

static DEFINITION: LazyLock<BotCommandDefinition> = LazyLock::new(MemoryCmd::definition);

pub struct MemoryCmd {
    pub memory_store: Arc<MemoryStore>,
    pub embedding_model: Arc<dyn EmbeddingModel + Send + Sync>,
}

#[async_trait]
impl CommandHandler<AgentRuntime> for MemoryCmd {
    fn definition(&self) -> &BotCommandDefinition {
        &DEFINITION
    }

    async fn execute(
        &self,
        client: Client<AgentRuntime, BotCommandContext>,
    ) -> Result<SuccessResult, String> {
        let action = client.context().command.arg::<String>("action").to_string();
        let content = client.context().command.arg::<String>("content").to_string();
        
        info!("Processing memory command with action: {} and content: {}", action, content);
        
        // Extract chat and user information based on scope type
        let scope = &client.context().scope;
        let (chat_id, user_id) = match scope {
            BotCommandScope::Chat(chat_details) => {
                // For chat scope, extract chat info and user info
                let chat_string = format!("{:?}", chat_details.chat);
                (chat_string, client.context().command.initiator.to_string())
            },
            BotCommandScope::Community(community_details) => {
                // For community scope, use community id
                let community_string = format!("{:?}", community_details.community_id);
                (community_string, client.context().command.initiator.to_string())
            },
        };
        
        let result = match action.as_str() {
            "store" => self.store_memory(chat_id, user_id, content).await,
            "recall" => self.recall_memory(chat_id, content).await,
            _ => Err(format!("Unknown memory action: {}", action)),
        };
        
        let response = match result {
            Ok(message) => message,
            Err(e) => {
                error!("Error processing memory command: {}", e);
                format!("I encountered an error: {}", e)
            }
        };
        
        let message = client
            .send_text_message(response)
            .with_block_level_markdown(true)
            .execute_then_return_message(|_, _| ());

        Ok(SuccessResult { message })
    }
}

impl MemoryCmd {
    fn definition() -> BotCommandDefinition {
        BotCommandDefinition {
            name: "memory".to_string(),
            description: Some("Store or recall information from KarmaSpark's memory".to_string()),
            placeholder: Some("Processing memory...".to_string()),
            params: vec![
                BotCommandParam {
                    name: "action".to_string(),
                    description: Some("Whether to store or recall a memory".to_string()),
                    placeholder: Some("Choose an action".to_string()),
                    required: true,
                    param_type: BotCommandParamType::StringParam(StringParam {
                        min_length: 1,
                        max_length: 10,
                        choices: vec![
                            BotCommandOptionChoice { 
                                name: "store".to_string(), 
                                value: "store".to_string() 
                            },
                            BotCommandOptionChoice { 
                                name: "recall".to_string(), 
                                value: "recall".to_string() 
                            }
                        ],
                        multi_line: false,
                    }),
                },
                BotCommandParam {
                    name: "content".to_string(),
                    description: Some("The memory to store or keywords to recall".to_string()),
                    placeholder: Some("Enter memory content or search terms".to_string()),
                    required: true,
                    param_type: BotCommandParamType::StringParam(StringParam {
                        min_length: 1,
                        max_length: 1000,
                        choices: Vec::new(),
                        multi_line: true,
                    }),
                },
            ],
            permissions: BotPermissions::from_message_permission(MessagePermission::Text),
            default_role: None,
            direct_messages: Some(true),
        }
    }
    
    async fn store_memory(&self, chat_id: String, user_id: String, content: String) -> Result<String, String> {
        // Create embedding for the memory
        let embedding = match self.embedding_model.embed_text(&content).await {
            Ok(embed) => Some(embed),
            Err(e) => {
                error!("Failed to create embedding: {}", e);
                None
            }
        };
        
        // Create the memory object
        let memory = Memory {
            id: None,
            chat_id,
            user_id,
            timestamp: Utc::now(),
            content: content.clone(),
            embedding,
            metadata: None,
        };
        
        // Store the memory
        match self.memory_store.store_memory(memory).await {
            Ok(_) => {
                info!("Memory stored successfully");
                Ok("I've stored this information in my memory.".to_string())
            }
            Err(e) => {
                error!("Failed to store memory: {}", e);
                Err(format!("Failed to store memory: {}", e))
            }
        }
    }
    
    async fn recall_memory(&self, chat_id: String, query: String) -> Result<String, String> {
        // First, try to create an embedding for semantic search
        let embedding_result = self.embedding_model.embed_text(&query).await;
        
        let memories: Vec<String> = match embedding_result {
            Ok(query_embedding) => {
                // Try semantic search first
                match self.memory_store.search_similar_memories(&chat_id, &query_embedding, 5).await {
                    Ok(results) if !results.is_empty() => {
                        // Found memories with semantic search
                        results.into_iter().map(|(m, score)| {
                            format!(
                                "- [{}] (similarity: {:.2}): {}",
                                m.timestamp.format("%Y-%m-%d %H:%M"),
                                score,
                                m.content
                            )
                        }).collect()
                    }
                    _ => {
                        // Fall back to recent memories
                        match self.memory_store.get_recent_memories(&chat_id, 5).await {
                            Ok(recent) => {
                                recent.into_iter().map(|m| {
                                    format!(
                                        "- [{}]: {}",
                                        m.timestamp.format("%Y-%m-%d %H:%M"),
                                        m.content
                                    )
                                }).collect()
                            }
                            Err(e) => {
                                return Err(format!("Failed to get recent memories: {}", e));
                            }
                        }
                    }
                }
            }
            Err(_) => {
                // Fall back to recent memories if embedding fails
                match self.memory_store.get_recent_memories(&chat_id, 5).await {
                    Ok(recent) => {
                        recent.into_iter().map(|m| {
                            format!(
                                "- [{}]: {}",
                                m.timestamp.format("%Y-%m-%d %H:%M"),
                                m.content
                            )
                        }).collect()
                    }
                    Err(e) => {
                        return Err(format!("Failed to get recent memories: {}", e));
                    }
                }
            }
        };
        
        if memories.is_empty() {
            Ok("I don't have any relevant memories for that query.".to_string())
        } else {
            let response = format!(
                "Here's what I remember about '{}':\n\n{}",
                query,
                memories.join("\n")
            );
            Ok(response)
        }
    }
} 