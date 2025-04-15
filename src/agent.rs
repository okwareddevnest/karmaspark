use anyhow::{anyhow, Result};
use chrono::Utc;
use oc_bots_sdk::oc_api::client::Client;
use oc_bots_sdk::types::{BotCommandContext, BotCommandScope};
use oc_bots_sdk_offchain::AgentRuntime;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::llm::{ChatMessage, MistralClient};

// ReAct planning stages
#[derive(Debug, Clone, PartialEq, Eq)]
enum PlanningState {
    Start,
    Thinking,
    Acting,
    Observing,
    Finished,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thought {
    pub id: String,
    pub content: String,
    pub timestamp: chrono::DateTime<Utc>,
}

impl Thought {
    fn new(content: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            content,
            timestamp: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAction {
    pub id: String,
    pub action_type: String,
    pub parameters: serde_json::Value,
    pub timestamp: chrono::DateTime<Utc>,
}

impl AgentAction {
    fn new(action_type: String, parameters: serde_json::Value) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            action_type,
            parameters,
            timestamp: Utc::now(),
        }
    }

    fn parse_from_llm_response(response: &str) -> Option<Self> {
        // For simple responses that could be direct answers, convert them to answer actions
        if !response.contains("ACTION:") {
            // If the response is short and simple, treat it as a direct answer
            if response.len() < 500 && !response.contains("PARAMETERS:") {
                // Create an answer action with the response as the final answer
                return Some(Self::new(
                    "answer".to_string(),
                    serde_json::json!({ "final_answer": response.trim() })
                ));
            }
            return None;
        }

        let action_parts: Vec<&str> = response.split("ACTION:").collect();
        if action_parts.len() < 2 {
            return None;
        }

        let action_text = action_parts[1].trim();
        let action_name_end = action_text.find('\n').unwrap_or(action_text.len());
        let action_name = action_text[..action_name_end].trim().to_string();
        
        // Extract parameters
        let mut parameters = serde_json::json!({});
        if let Some(params_start) = response.find("PARAMETERS:") {
            let params_text = &response[params_start + "PARAMETERS:".len()..];
            let params_end = params_text.find("\n\n").unwrap_or(params_text.len());
            let params_json = params_text[..params_end].trim();
            
            if let Ok(parsed) = serde_json::from_str(params_json) {
                parameters = parsed;
            }
        }

        Some(Self::new(action_name, parameters))
    }
}

impl fmt::Display for AgentAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Action: {}\nParameters: {}",
            self.action_type,
            serde_json::to_string_pretty(&self.parameters).unwrap_or_else(|_| "{}".to_string())
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    pub id: String,
    pub content: String,
    pub action_id: String,
    pub timestamp: chrono::DateTime<Utc>,
}

impl Observation {
    fn new(content: String, action_id: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            content,
            action_id,
            timestamp: Utc::now(),
        }
    }
}

// Configuration for the agent
#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub max_steps: usize,
    pub temperature: f32,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_steps: 3,
            temperature: 0.7,
        }
    }
}

#[derive(Clone)]
pub struct Agent {
    llm: MistralClient,
    config: AgentConfig,
}

impl Agent {
    pub fn new(llm: MistralClient) -> Self {
        Self {
            llm,
            config: AgentConfig::default(),
        }
    }

    pub fn with_config(mut self, config: AgentConfig) -> Self {
        self.config = config;
        self
    }
    
    pub async fn plan_and_execute(
        &self,
        client: &Client<AgentRuntime, BotCommandContext>,
        query: &str,
    ) -> Result<(String, Vec<String>)> {
        info!("Starting planning for query: {}", query);
        
        // Extract scope and context
        let scope = &client.context().scope;
        
        // Extract chat and user information based on scope type
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
        
        // For very simple queries, provide direct answers
        if query.len() < 10 && (
            query.to_lowercase().contains("hello") || 
            query.to_lowercase().contains("hi") || 
            query.to_lowercase().contains("hey")
        ) {
            return Ok((
                format!("Hello! How can I assist you today?"),
                vec![]
            ));
        }
        
        // Initialize planning state and tracking structures
        let mut state = PlanningState::Start;
        let mut thoughts: Vec<Thought> = Vec::new();
        let mut actions: Vec<AgentAction> = Vec::new();
        let mut observations: Vec<Observation> = Vec::new();
        let mut current_step = 0;
        let mut final_answer = String::new();
        let mut consecutive_thinking_count = 0;
        
        // Set up system prompt for ReAct planning
        let system_prompt = self.create_system_prompt(query);
        
        // Delay between LLM calls to avoid rate limits
        let delay_duration = Duration::from_secs(2);
        
        // Main planning loop
        while current_step < self.config.max_steps && state != PlanningState::Finished {
            match state {
                PlanningState::Start => {
                    // Initial thought
                    let initial_thought = format!(
                        "I need to help answer the user's question: \"{}\". Let me think about this step by step.",
                        query
                    );
                    thoughts.push(Thought::new(initial_thought));
                    state = PlanningState::Thinking;
                }
                
                PlanningState::Thinking => {
                    info!("Step {}: Thinking...", current_step + 1);
                    consecutive_thinking_count += 1;
                    
                    // If we've been in thinking state too many times, provide a fallback response
                    if consecutive_thinking_count > 5 {
                        info!("Too many consecutive thinking steps, providing fallback answer");
                        
                        if !observations.is_empty() {
                            final_answer = self.generate_partial_answer_from_observations(&observations, query).await?.0;
                        } else {
                            // Fallback to a direct answer attempt
                            final_answer = format!(
                                "I've thought about your question \"{}\" and here's my answer:\n\n",
                                query
                            );
                            
                            let simple_prompt = format!(
                                "You are a helpful assistant. Provide a direct, concise answer to this question: \"{}\"",
                                query
                            );
                            
                            let direct_response = match self.llm.chat(&simple_prompt, &[]).await {
                                Ok(response) => response,
                                Err(_) => "I'm not able to provide a complete answer at this time. Please try asking your question differently.".to_string(),
                            };
                            
                            final_answer.push_str(&direct_response);
                        }
                        
                        state = PlanningState::Finished;
                        continue;
                    }
                    
                    // Add delay before making LLM call to avoid rate limits
                    sleep(delay_duration).await;
                    
                    // Generate current context for LLM
                    let messages = self.build_message_history(&thoughts, &actions, &observations);
                    
                    // Get next step from LLM
                    let response = match self.llm.chat(&system_prompt, &messages).await {
                        Ok(response) => response,
                        Err(e) => {
                            error!("Error getting LLM response: {}", e);
                            // If we hit an error but have observations, try to provide a partial answer
                            if !observations.is_empty() {
                                return self.generate_partial_answer_from_observations(&observations, query).await;
                            }
                            return Err(anyhow!("Failed to get LLM response: {}", e));
                        }
                    };
                    
                    // Parse response to determine next state
                    if let Some(action) = AgentAction::parse_from_llm_response(&response) {
                        // Reset consecutive thinking counter when we get an action
                        consecutive_thinking_count = 0;
                        
                        // Handle 'answer' action separately as it's the exit condition
                        if action.action_type == "answer" {
                            if let Some(answer) = action.parameters.get("final_answer") {
                                final_answer = answer.as_str().unwrap_or("").to_string();
                                state = PlanningState::Finished;
                                
                                // Record this as the final thought
                                thoughts.push(Thought::new(
                                    format!("I now have the answer: {}", final_answer)
                                ));
                            } else {
                                warn!("Answer action without final_answer parameter");
                                thoughts.push(Thought::new("I need to provide a clear answer".to_string()));
                            }
                        } else {
                            // Record thought and action for other action types
                            let thought_content = format!("I need to {}", action.action_type);
                            thoughts.push(Thought::new(thought_content));
                            actions.push(action);
                            state = PlanningState::Acting;
                        }
                    } else {
                        // Treat response as a thought if it's not an action
                        thoughts.push(Thought::new(response));
                        // Stay in thinking state
                    }
                }
                
                PlanningState::Acting => {
                    if let Some(action) = actions.last() {
                        info!("Step {}: Acting - {}", current_step + 1, action.action_type);
                        
                        // Add delay before making any potential LLM calls in execute_action
                        sleep(delay_duration).await;
                        
                        // Perform the action
                        match self.execute_action(action, chat_id.clone(), user_id.clone()).await {
                            Ok(result) => {
                                // Record observation
                                let observation = Observation::new(result, action.id.clone());
                                observations.push(observation);
                                state = PlanningState::Observing;
                            },
                            Err(e) => {
                                error!("Error executing action: {}", e);
                                let error_observation = Observation::new(
                                    format!("Error: {}", e), 
                                    action.id.clone()
                                );
                                observations.push(error_observation);
                                state = PlanningState::Observing;
                            }
                        }
                    } else {
                        error!("No action to execute, this shouldn't happen");
                        state = PlanningState::Thinking;
                    }
                }
                
                PlanningState::Observing => {
                    debug!("Step {}: Observing results", current_step + 1);
                    // After observation, go back to thinking
                    state = PlanningState::Thinking;
                    current_step += 1;
                }
                
                PlanningState::Finished => {
                    // Should not reach here normally, as the loop condition would exit
                    debug!("Planning finished with answer: {}", final_answer);
                    break;
                }
            }
        }
        
        // If we reached max steps without finishing, provide a reasonable answer
        if state != PlanningState::Finished {
            info!("Reached maximum steps without final answer, generating summary");
            final_answer = self.generate_final_answer(&thoughts, &actions, &observations, query).await?;
        }
        
        // Collect observations for return
        let observation_texts = observations.iter()
            .map(|o| o.content.clone())
            .collect();
        
        Ok((final_answer, observation_texts))
    }

    // New helper method to generate a partial answer from observations if we hit an error
    async fn generate_partial_answer_from_observations(
        &self,
        observations: &[Observation],
        query: &str,
    ) -> Result<(String, Vec<String>)> {
        let mut answer = format!(
            "I encountered an issue while processing your question about '{}', but here's what I found so far:\n\n",
            query
        );
        
        // Add all observations
        for (i, observation) in observations.iter().enumerate() {
            answer.push_str(&format!("Finding {}: {}\n\n", i + 1, observation.content));
        }
        
        answer.push_str("\nI apologize that I couldn't complete the full analysis due to technical limitations.");
        
        // Return the partial answer and observations
        let observation_texts = observations.iter()
            .map(|o| o.content.clone())
            .collect();
            
        Ok((answer, observation_texts))
    }

    // Helper function to create the system prompt
    fn create_system_prompt(&self, query: &str) -> String {
        format!(
            "You are KarmaSpark, an intelligent assistant capable of step-by-step problem solving. You will think carefully before taking actions.\n\
            The user has asked: \"{}\"\n\n\
            To solve this, you should follow a structured approach:\n\
            1. Think about what you know and what information you need\n\
            2. Decide what action to take\n\
            3. Observe the result\n\
            4. Plan your next step or provide a final answer\n\n\
            When you need to take an action, respond using EXACTLY this format:\n\
            ACTION: <action_name>\n\
            PARAMETERS: {{\"parameter_name\": \"parameter_value\"}}\n\n\
            For example, to search for information:\n\
            ACTION: search_information\n\
            PARAMETERS: {{\"query\": \"history of chess\"}}\n\n\
            To provide a final answer:\n\
            ACTION: answer\n\
            PARAMETERS: {{\"final_answer\": \"Your complete answer here\"}}\n\n\
            Valid actions are:\n\
            - search_information: {{\"query\": \"search terms\"}}\n\
            - perform_calculation: {{\"expression\": \"math expression\"}}\n\
            - answer: {{\"final_answer\": \"your final answer to the user\"}}\n\n\
            IMPORTANT: For simple questions, you can immediately use the answer action without other steps.\n\
            Do not include any narrative text outside of the specified format.",
            query
        )
    }

    // Helper function to build the conversation history for the LLM
    fn build_message_history(
        &self, 
        thoughts: &[Thought], 
        actions: &[AgentAction], 
        observations: &[Observation]
    ) -> Vec<ChatMessage> {
        let mut messages = Vec::new();
        
        // Add thoughts, actions, and observations as conversational history
        for (i, thought) in thoughts.iter().enumerate() {
            messages.push(ChatMessage {
                role: "assistant".to_string(),
                content: format!("Thought {}: {}", i + 1, thought.content),
            });
            
            // If there's a corresponding action and observation
            if i < actions.len() {
                messages.push(ChatMessage {
                    role: "assistant".to_string(),
                    content: format!("Action {}: {}", i + 1, actions[i]),
                });
                
                if i < observations.len() {
                    messages.push(ChatMessage {
                        role: "user".to_string(),
                        content: format!("Observation {}: {}", i + 1, observations[i].content),
                    });
                }
            }
        }
        
        // Ask the LLM for the next step
        let next_step_prompt = if messages.is_empty() {
            "What is your first step to solve this problem?".to_string()
        } else {
            "What is your next step? You can either think more about the problem, take an action, or provide your final answer.".to_string()
        };
        
        messages.push(ChatMessage {
            role: "user".to_string(),
            content: next_step_prompt,
        });

        messages
    }

    // Generate a final answer if we reached max steps
    async fn generate_final_answer(
        &self,
        thoughts: &[Thought],
        actions: &[AgentAction],
        observations: &[Observation],
        query: &str,
    ) -> Result<String> {
        let system_prompt = format!(
            "You are KarmaSpark, an intelligent assistant. Based on the following thought process and observations, \
            provide a concise and helpful answer to the user's question: \"{}\". \
            Focus on giving the most useful information you've gathered so far.",
            query
        );

        let mut messages = Vec::new();
        
        // Add all thoughts, actions, and observations
        for (i, thought) in thoughts.iter().enumerate() {
            messages.push(ChatMessage {
                role: "assistant".to_string(),
                content: format!("Thought {}: {}", i + 1, thought.content),
            });
            
            if i < actions.len() {
                messages.push(ChatMessage {
                    role: "assistant".to_string(),
                    content: format!("Action {}: {}", i + 1, actions[i]),
                });
                
                if i < observations.len() {
                    messages.push(ChatMessage {
                        role: "user".to_string(),
                        content: format!("Observation {}: {}", i + 1, observations[i].content),
                    });
                }
            }
        }
        
        messages.push(ChatMessage {
            role: "user".to_string(),
            content: "Based on all the information you've gathered, what's your final answer to my question?".to_string(),
        });
        
        match self.llm.chat(&system_prompt, &messages).await {
            Ok(answer) => Ok(answer),
            Err(e) => {
                error!("Error generating final answer: {}", e);
                Ok("I wasn't able to find a complete answer to your question in the time available.".to_string())
            }
        }
    }
    
    async fn execute_action(
        &self,
        action: &AgentAction,
        _chat_id: String,
        _user_id: String, 
    ) -> Result<String> {
        match action.action_type.as_str() {
            "search_information" => {
                let query = action.parameters.get("query")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("No search query provided"))?;
                
                if query.is_empty() {
                    return Ok("No search query provided.".to_string());
                }
                
                // Simulate search (in a real system, this would call a search API)
                let search_prompt = format!(
                    "You are a search engine. Provide a brief, factual answer to this query: \"{}\"",
                    query
                );
                
                let messages = vec![
                    ChatMessage {
                        role: "user".to_string(),
                        content: query.to_string(),
                    }
                ];
                
                // Use the LLM as a simulated search engine
                match self.llm.chat(&search_prompt, &messages).await {
                    Ok(result) => Ok(result),
                    Err(e) => Err(anyhow!("Search error: {}", e)),
                }
            },
            
            "perform_calculation" => {
                let expression = action.parameters.get("expression")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("No calculation expression provided"))?;
                
                if expression.is_empty() {
                    return Ok("No calculation expression provided.".to_string());
                }
                
                // Use LLM to evaluate the expression (in production, you'd want a proper math engine)
                let calc_prompt = format!(
                    "You are a calculator. Compute the result of this expression: \"{}\". \
                    Return only the numeric result without explanation.",
                    expression
                );
                
                let messages = vec![
                    ChatMessage {
                        role: "user".to_string(),
                        content: expression.to_string(),
                    }
                ];
                
                match self.llm.chat(&calc_prompt, &messages).await {
                    Ok(result) => Ok(result),
                    Err(e) => Err(anyhow!("Calculation error: {}", e)),
                }
            },
            
            _ => Err(anyhow!("Unsupported action: {}", action.action_type)),
        }
    }
}