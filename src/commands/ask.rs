use async_trait::async_trait;
use oc_bots_sdk::api::command::{CommandHandler, SuccessResult};
use oc_bots_sdk::api::definition::*;
use oc_bots_sdk::types::BotCommandContext;
use oc_bots_sdk_offchain::AgentRuntime;
use oc_bots_sdk::oc_api::client::Client;
use std::sync::LazyLock;
use std::sync::Arc;
use tracing::{error, info};

use crate::agent::Agent;

static DEFINITION: LazyLock<BotCommandDefinition> = LazyLock::new(Ask::definition);

pub struct Ask {
    pub agent: Arc<Agent>,
}

#[async_trait]
impl CommandHandler<AgentRuntime> for Ask {
    fn definition(&self) -> &BotCommandDefinition {
        &DEFINITION
    }

    async fn execute(
        &self,
        client: Client<AgentRuntime, BotCommandContext>,
    ) -> Result<SuccessResult, String> {
        let query = client.context().command.arg::<String>("query").to_string();
        
        info!("Processing ask command with query: {}", query);
        
        // Call agent to plan and execute based on query
        let (response, _observations) = match self.agent.plan_and_execute(&client, &query).await {
            Ok((answer, obs)) => (answer, obs),
            Err(e) => {
                error!("Agent error: {}", e);
                (format!("I'm sorry, I encountered an error: {}", e), Vec::new())
            }
        };
        
        info!("Ask command response: {}", response);
        
        let message = client
            .send_text_message(response)
            .with_block_level_markdown(true)
            .execute_then_return_message(|_, _| ());

        Ok(SuccessResult { message })
    }
}

impl Ask {
    fn definition() -> BotCommandDefinition {
        BotCommandDefinition {
            name: "ask".to_string(),
            description: Some("Ask KarmaSpark a question and get an intelligent response".to_string()),
            placeholder: Some("Thinking...".to_string()),
            params: vec![BotCommandParam {
                name: "query".to_string(),
                description: Some("Your question or request".to_string()),
                placeholder: Some("What would you like to know?".to_string()),
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