use async_trait::async_trait;
use oc_bots_sdk::api::command::{CommandHandler, SuccessResult};
use oc_bots_sdk::api::definition::*;
use oc_bots_sdk::types::BotCommandContext;
use oc_bots_sdk_offchain::AgentRuntime;
use oc_bots_sdk::oc_api::client::Client;
use std::sync::LazyLock;
use std::sync::Arc;
use tracing::{error, info};

use crate::llm::MistralClient;

static DEFINITION: LazyLock<BotCommandDefinition> = LazyLock::new(Moderate::definition);

pub struct Moderate {
    pub llm: Arc<MistralClient>,
}

#[async_trait]
impl CommandHandler<AgentRuntime> for Moderate {
    fn definition(&self) -> &BotCommandDefinition {
        &DEFINITION
    }

    async fn execute(
        &self,
        client: Client<AgentRuntime, BotCommandContext>,
    ) -> Result<SuccessResult, String> {
        let content = client.context().command.arg::<String>("content").to_string();
        
        info!("Processing moderation request for content: {}", content);
        
        // Use the LLM to moderate the content
        let moderation_result = match self.llm.moderate(&content).await {
            Ok((is_flagged, reason)) => {
                if is_flagged {
                    format!("⚠️ **Content flagged**\n\nReason: {}", reason)
                } else {
                    "✅ **Content safe**\n\nNo harmful content detected.".to_string()
                }
            }
            Err(e) => {
                error!("Error moderating content: {}", e);
                format!("I encountered an error while moderating: {}", e)
            }
        };
        
        let message = client
            .send_text_message(moderation_result)
            .with_block_level_markdown(true)
            .execute_then_return_message(|_, _| ());

        Ok(SuccessResult { message })
    }
}

impl Moderate {
    fn definition() -> BotCommandDefinition {
        BotCommandDefinition {
            name: "moderate".to_string(),
            description: Some("Check if content contains harmful or inappropriate material".to_string()),
            placeholder: Some("Analyzing content...".to_string()),
            params: vec![BotCommandParam {
                name: "content".to_string(),
                description: Some("The content to check for harmful material".to_string()),
                placeholder: Some("Enter the content to moderate".to_string()),
                required: true,
                param_type: BotCommandParamType::StringParam(StringParam {
                    min_length: 1,
                    max_length: 10000,
                    choices: Vec::new(),
                    multi_line: true,
                }),
            }],
            permissions: BotPermissions::from_message_permission(MessagePermission::Text),
            default_role: None,
            direct_messages: Some(true),
        }
    }
} 