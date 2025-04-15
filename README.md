# KarmaSpark - Agentic AI Bot for OpenChat

KarmaSpark is an intelligent assistant bot for OpenChat built on the OpenChat Bot SDK. It provides agentic AI capabilities, memory, planning, and other advanced features to enhance your chat experience.

## Features

- **Intelligent Agent**: Ask complex questions and get well-researched answers
- **Memory**: Bot remembers previous conversations and can retrieve them later
- **Reminders**: Set reminders for future tasks or events
- **Summarization**: Get concise summaries of text or conversations
- **Moderation**: Content moderation capabilities to ensure safe interactions
- **Advanced Planning**: Multi-step planning for complex problem-solving

## Commands

KarmaSpark offers several commands:

- `/ask [query]`: Ask the agent any question and get an intelligent response
- `/memory [query]`: Search your conversation history or save important information
- `/remindme [minutes] [message]`: Set a reminder for a future time
- `/summarize [text]`: Generate a concise summary of provided text
- `/moderate [text]`: Check if content contains inappropriate material
- `/echo [message]`: Simple echo command that repeats your message

## Setup Guide

### Prerequisites
- Rust and Cargo installed
- OpenChat Bot SDK
- Mistral API key for LLM functionality

### Configuration

1. **Clone the repository**
   ```bash
   git clone https://github.com/okwareddevnest/karmaspark.git
   cd karmaspark
   ```

2. **Set up environment**
   Create a `.env` file with your Mistral API key:
   ```
   MISTRAL_API_KEY=your-api-key-here
   ```
   
   Alternatively, add it to `config.toml`:
   ```toml
   mistral_api_key = "your-api-key-here"
   ```

3. **Additional configuration**
   Edit `config.toml` to configure:
   - Agent capabilities (memory, planning, moderation)
   - Server port
   - Log level
   - Memory retention settings

### Running the Bot

```bash
cd src
cargo run
```

The bot will start an HTTP server on the configured port (default: 8080).

## Architecture

KarmaSpark is built on a modular architecture:

- **Agent**: Core reasoning and planning capabilities
- **Memory**: SQLite-based storage for conversation history
- **LLM Integration**: Mistral AI integration for natural language understanding
- **Command Handlers**: Modular command implementation
- **HTTP Server**: Axum-based server for OpenChat integration

## Development

### Adding New Commands

Follow the existing command pattern in the `src/commands` directory.

### Extending Agent Capabilities

The agent logic is in `src/agent.rs` and can be extended with new capabilities.

## License

This project is licensed under the MIT License. See the LICENSE file for details.

## Acknowledgments

Built on the [OpenChat Bot SDK](https://github.com/open-chat-labs/open-chat-bots) and inspired by the offchain bot example.