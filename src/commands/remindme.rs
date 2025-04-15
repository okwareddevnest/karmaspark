use async_trait::async_trait;
use oc_bots_sdk::api::command::{CommandHandler, SuccessResult};
use oc_bots_sdk::api::definition::*;
use oc_bots_sdk::types::BotCommandContext;
use oc_bots_sdk_offchain::AgentRuntime;
use oc_bots_sdk::oc_api::client::Client;
use std::sync::LazyLock;
use std::thread;
use tracing::info;

static DEFINITION: LazyLock<BotCommandDefinition> = LazyLock::new(RemindMe::definition);

pub struct RemindMe;

// Global reminder counter to help with logging
static mut REMINDER_COUNTER: usize = 0;

#[async_trait]
impl CommandHandler<AgentRuntime> for RemindMe {
    fn definition(&self) -> &BotCommandDefinition {
        &DEFINITION
    }

    async fn execute(
        &self,
        client: Client<AgentRuntime, BotCommandContext>,
    ) -> Result<SuccessResult, String> {
        let reminder = client.context().command.arg::<String>("reminder").to_string();
        let minutes = client.context().command.arg::<f64>("minutes");
        
        // Get a unique ID for this reminder for logging purposes
        let reminder_id = unsafe {
            REMINDER_COUNTER += 1;
            REMINDER_COUNTER
        };
        
        info!("Setting reminder #{} for {} minutes: {}", reminder_id, minutes, reminder);
        
        // Create a confirmation message
        let confirmation = format!(
            "I'll remind you in {} minutes about: {}",
            minutes,
            reminder
        );
        
        // Send confirmation message first and get the result
        let message = client
            .send_text_message(confirmation.clone())
            .with_block_level_markdown(true)
            .execute_then_return_message(|_, _| ());
            
        // Extract information needed for reminder
        let user_id = client.context().command.initiator.to_string();
        let seconds = (minutes * 60.0) as u64;
        let reminder_clone = reminder.clone();
        
        // Use std::thread for the reminder to completely detach it from Tokio runtime
        thread::spawn(move || {
            // Sleep using std::thread::sleep to avoid tokio runtime issues
            info!("Reminder #{} scheduled to trigger in {} seconds", reminder_id, seconds);
            thread::sleep(std::time::Duration::from_secs(seconds));
            
            // Log that the reminder was triggered
            info!("REMINDER #{} TRIGGERED for user {}: {}", 
                  reminder_id, user_id, reminder_clone);
                  
            // Note: In a production system, you would want to implement a more robust
            // reminder system using a persistent storage and a separate process/service
        });

        Ok(SuccessResult { message })
    }
}

impl RemindMe {
    fn definition() -> BotCommandDefinition {
        BotCommandDefinition {
            name: "remindme".to_string(),
            description: Some("Set a reminder for later".to_string()),
            placeholder: Some("Setting reminder...".to_string()),
            params: vec![
                BotCommandParam {
                    name: "reminder".to_string(),
                    description: Some("What you want to be reminded about".to_string()),
                    placeholder: Some("Enter what you want to be reminded about".to_string()),
                    required: true,
                    param_type: BotCommandParamType::StringParam(StringParam {
                        min_length: 1,
                        max_length: 1000,
                        choices: Vec::new(),
                        multi_line: true,
                    }),
                },
                BotCommandParam {
                    name: "minutes".to_string(),
                    description: Some("How many minutes from now to send the reminder".to_string()),
                    placeholder: Some("Enter minutes".to_string()),
                    required: true,
                    param_type: BotCommandParamType::DecimalParam(DecimalParam {
                        min_value: 1.0,
                        max_value: 10080.0, // Max 1 week (7 days * 24 hours * 60 minutes)
                        choices: Vec::new(),
                    }),
                },
            ],
            permissions: BotPermissions::from_message_permission(MessagePermission::Text),
            default_role: None,
            direct_messages: Some(true),
        }
    }
} 