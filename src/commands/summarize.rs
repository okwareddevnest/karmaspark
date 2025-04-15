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

static DEFINITION: LazyLock<BotCommandDefinition> = LazyLock::new(Summarize::definition);

pub struct Summarize {
    pub llm: Arc<MistralClient>,
}

#[async_trait]
impl CommandHandler<AgentRuntime> for Summarize {
    fn definition(&self) -> &BotCommandDefinition {
        &DEFINITION
    }

    async fn execute(
        &self,
        client: Client<AgentRuntime, BotCommandContext>,
    ) -> Result<SuccessResult, String> {
        let text = client.context().command.arg::<String>("text").to_string();
        
        info!("Processing summarize command with text of length: {}", text.len());
        
        // Use the LLM to summarize the text
        let summary = match self.llm.summarize(&text).await {
            Ok(summary) => summary,
            Err(e) => {
                error!("Error summarizing text: {}", e);
                format!("I encountered an error while summarizing: {}", e)
            }
        };
        
        info!("Summary generated of length: {}", summary.len());
        
        let message = client
            .send_text_message(format!("**Summary:**\n\n{}", summary))
            .with_block_level_markdown(true)
            .execute_then_return_message(|_, _| ());

        Ok(SuccessResult { message })
    }
}

impl Summarize {
    fn definition() -> BotCommandDefinition {
        BotCommandDefinition {
            name: "summarize".to_string(),
            description: Some("Summarize a block of text or a discussion".to_string()),
            placeholder: Some("Summarizing...".to_string()),
            params: vec![BotCommandParam {
                name: "text".to_string(),
                description: Some("The text to summarize".to_string()),
                placeholder: Some("Paste the text you want to summarize".to_string()),
                required: true,
                param_type: BotCommandParamType::StringParam(StringParam {
                    min_length: 10,
                    max_length: 50000,
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