use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Router,
};
use dotenv::dotenv;
use oc_bots_sdk::api::command::{CommandHandlerRegistry, CommandResponse};
use oc_bots_sdk::api::definition::BotDefinition;
use oc_bots_sdk::oc_api::client::ClientFactory;
use oc_bots_sdk_offchain::{env, AgentRuntime};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::{info, error};
use tracing_subscriber::fmt::format::FmtSpan;

mod config;
mod commands;
mod memory;
mod llm;
mod agent;

use crate::agent::Agent;
use crate::llm::{MistralClient, MistralEmbedding};
use crate::memory::MemoryStore;

// Structure to hold application state
struct AppState {
    oc_public_key: String,
    commands: CommandHandlerRegistry<AgentRuntime>,
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // Load .env file if present
    dotenv().ok();

    // Get config file path from env - if not set, use default
    let config_file_path = std::env::var("CONFIG_FILE").unwrap_or("./config.toml".to_string());
    println!("Config file path: {:?}", config_file_path);

    // Load & parse config
    let config = config::Config::from_file(&config_file_path).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to load config file: {}", e),
        )
    })?;
    println!("Config: {:?}", config);

    // Setup logging
    tracing_subscriber::fmt()
        .with_max_level(config.log_level)
        .with_span_events(FmtSpan::CLOSE)
        .init();

    info!("Starting KarmaSpark bot for OpenChat");

    // Get Mistral API key
    let mistral_api_key = match config.mistral_api_key() {
        Ok(key) => key,
        Err(e) => {
            error!("Failed to get Mistral API key: {}", e);
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Mistral API key not found. Please set it in config.toml or MISTRAL_API_KEY environment variable.",
            ));
        }
    };

    // Initialize LLM client
    let llm_client = Arc::new(MistralClient::new(&mistral_api_key));
    
    // Initialize embedding model
    let embedding_model = Arc::new(MistralEmbedding::new(&mistral_api_key));
    
    // Initialize memory store if enabled
    let memory_store = if config.agent.enable_memory {
        let db_path = config.sqlite_db_path.clone().unwrap_or("./karmaspark.db".to_string());
        match MemoryStore::new(&db_path) {
            Ok(store) => {
                info!("Memory store initialized with database at {}", db_path);
                Some(Arc::new(store))
            }
            Err(e) => {
                error!("Failed to initialize memory store: {}", e);
                None
            }
        }
    } else {
        info!("Memory store disabled in config");
        None
    };
    
    // Initialize agent
    let agent = Arc::new(Agent::new(
        llm_client.as_ref().clone(),
    ));

    // Build agent for OpenChat communication
    let oc_agent = oc_bots_sdk_offchain::build_agent(config.ic_url.clone(), &config.pem_file).await;

    // Create runtime and client factory
    let runtime = AgentRuntime::new(oc_agent, tokio::runtime::Runtime::new().map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to create tokio runtime: {}", e)
        )
    })?);
    let client_factory = Arc::new(ClientFactory::new(runtime));

    // Create command registry and register commands
    let mut command_registry = CommandHandlerRegistry::new(client_factory);
    
    // Register the original echo command
    command_registry = command_registry.register(commands::echo::Echo);
    
    // Register new commands
    
    // Ask command
    command_registry = command_registry.register(commands::ask::Ask {
        agent: agent.clone(),
    });
    
    // Summarize command
    command_registry = command_registry.register(commands::summarize::Summarize {
        llm: llm_client.clone(),
    });
    
    // RemindMe command
    command_registry = command_registry.register(commands::remindme::RemindMe);
    
    // Moderate command
    if config.agent.enable_moderation {
        command_registry = command_registry.register(commands::moderate::Moderate {
            llm: llm_client.clone(),
        });
    }
    
    // Memory command
    if config.agent.enable_memory && memory_store.is_some() {
        command_registry = command_registry.register(commands::memory::MemoryCmd {
            memory_store: memory_store.clone().unwrap(),
            embedding_model: embedding_model,
        });
    }

    let app_state = AppState {
        oc_public_key: config.oc_public_key,
        commands: command_registry,
    };

    // Create router with endpoints
    let app = Router::new()
        .route("/", get(bot_definition))
        .route("/bot_definition", get(bot_definition))
        .route("/execute", post(execute_command))
        .route("/execute_command", post(execute_command))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(Arc::new(app_state));

    // Start HTTP server
    let socket_addr = SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), config.port);
    info!("Starting HTTP server on {}", socket_addr);
    
    let listener = tokio::net::TcpListener::bind(socket_addr).await?;
    
    // Simplify with ? operator
    axum::serve(listener, app.into_make_service()).await?;
    
    Ok(())
}

// Bot definition endpoint
async fn bot_definition(State(state): State<Arc<AppState>>) -> (StatusCode, Bytes) {
    let commands = state.commands.definitions();
    
    let definition = BotDefinition {
        description: "KarmaSpark - An agentic AI bot with memory, planning, and intelligent assistance".to_string(),
        commands,
        autonomous_config: None,
    };

    (
        StatusCode::OK,
        Bytes::from(serde_json::to_vec(&definition).unwrap()),
    )
}

// Command execution endpoint
async fn execute_command(
    State(state): State<Arc<AppState>>, 
    headers: HeaderMap,
) -> (StatusCode, Bytes) {
    info!("=== Command Execution Start ===");
    info!("Headers: {:?}", headers);
    
    // Get JWT from x-oc-jwt header
    let jwt = match headers.get("x-oc-jwt") {
        Some(jwt_header) => {
            match jwt_header.to_str() {
                Ok(jwt) => {
                    info!("Found JWT in x-oc-jwt header");
                    jwt.to_string()
                },
                Err(e) => {
                    error!("Invalid JWT header value: {}", e);
                    return (
                        StatusCode::BAD_REQUEST,
                        Bytes::from("Invalid JWT header value"),
                    );
                }
            }
        },
        None => {
            error!("No JWT found in x-oc-jwt header");
            return (
                StatusCode::BAD_REQUEST,
                Bytes::from("Missing JWT header"),
            );
        }
    };

    info!("JWT length: {}", jwt.len());
    
    // Parse command data from the JWT payload
    let result = state
        .commands
        .execute(&jwt, &state.oc_public_key, env::now())
        .await;
        
    info!("Command execution result: {:?}", result);
    info!("=== Command Execution End ===");
    
    match result {
        CommandResponse::Success(r) => {
            info!("Command executed successfully");
            (StatusCode::OK, Bytes::from(serde_json::to_vec(&r).unwrap()))
        }
        CommandResponse::BadRequest(r) => {
            error!("Bad request: {:?}", r);
            (
                StatusCode::BAD_REQUEST,
                Bytes::from(serde_json::to_vec(&r).unwrap()),
            )
        }
        CommandResponse::InternalError(err) => {
            error!("Internal error: {:?}", err);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Bytes::from(format!("{err:?}")),
            )
        }
        CommandResponse::TooManyRequests => {
            error!("Too many requests");
            (StatusCode::TOO_MANY_REQUESTS, Bytes::new())
        }
    }
}