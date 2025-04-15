#!/bin/bash

# KarmaSpark - OpenChat Bot Setup Script
# Version: 1.5 (Agentic Edition)

# Text formatting
BOLD='\033[1m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BLUE='\033[0;34m'
RED='\033[0;31m'
NC='\033[0m' # No Color

# Config variables
IDENTITY_NAME="karmaspark_identity"
CONFIG_FILE="config.toml"
ENV_FILE=".env"
ENV_EXAMPLE_FILE=".env.example"
DB_PATH="./karmaspark.db"
PORT=13457

# Identify if we're in the src directory or the project root
if [ -d "src" ] && [ -f "README.md" ]; then
    # We're in the project root
    SRC_DIR="./src"
    CONFIG_DIR="./"
elif [ -f "Cargo.toml" ] && [ -d "src" ]; then
    # We're in the src directory
    SRC_DIR="."
    CONFIG_DIR="../"
    cd ..
else
    echo -e "${RED}Error: Script must be run from either the project root or the src directory${NC}"
    exit 1
fi

# Exit on error
set -e

# Banner
echo -e "${BOLD}${BLUE}"
echo "╔═══════════════════════════════════════════════════════════╗"
echo "║                                                           ║"
echo "║   🧠 KarmaSpark - Agentic AI Bot for OpenChat             ║"
echo "║       Setup and Deployment Script                         ║"
echo "║                                                           ║"
echo "╚═══════════════════════════════════════════════════════════╝"
echo -e "${NC}"

# Function to check if required utilities are installed
check_prerequisites() {
    echo -e "${BOLD}Checking prerequisites...${NC}"
    
    # Check for DFX
    if ! command -v dfx &> /dev/null; then
        echo -e "${RED}❌ dfx not found. Please install dfx first:${NC}"
        echo "   sh -ci \"$(curl -fsSL https://internetcomputer.org/install.sh)\""
        exit 1
    else
        echo -e "${GREEN}✓ dfx is installed${NC}"
    fi
    
    # Check for Rust/Cargo
    if ! command -v cargo &> /dev/null; then
        echo -e "${RED}❌ Rust/Cargo not found. Please install Rust first:${NC}"
        echo "   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
        exit 1
    else
        echo -e "${GREEN}✓ Rust/Cargo is installed${NC}"
    fi
    
    # Check for curl
    if ! command -v curl &> /dev/null; then
        echo -e "${RED}❌ curl not found. Please install curl first.${NC}"
        exit 1
    else
        echo -e "${GREEN}✓ curl is installed${NC}"
    fi
    
    echo ""
}

# Function to create and configure identity
setup_identity() {
    echo -e "${BOLD}Setting up identity...${NC}"
    
    # Check if identity already exists
    if dfx identity list | grep -q "$IDENTITY_NAME"; then
        echo -e "${YELLOW}⚠️ Identity '$IDENTITY_NAME' already exists.${NC}"
        read -p "Do you want to use the existing identity? (y/n): " use_existing
        if [[ $use_existing == "n" || $use_existing == "N" ]]; then
            echo "Removing existing identity..."
            dfx identity remove "$IDENTITY_NAME"
            dfx identity new "$IDENTITY_NAME" --storage-mode=plaintext
        fi
    else
        echo "Creating new identity '$IDENTITY_NAME'..."
        dfx identity new "$IDENTITY_NAME" --storage-mode=plaintext
    fi
    
    # Export identity to PEM file
    echo "Exporting identity to PEM file..."
    dfx identity export "$IDENTITY_NAME" > ${SRC_DIR}/identity.pem
    echo -e "${GREEN}✓ Identity setup complete${NC}"
    echo ""
}

# Function to fetch OpenChat public key
fetch_openchat_key() {
    echo -e "${BOLD}Fetching OpenChat public key...${NC}"
    OC_PUBLIC_KEY=$(curl -s https://oc.app/public-key)
    
    if [[ -z "$OC_PUBLIC_KEY" ]]; then
        echo -e "${RED}❌ Failed to fetch OpenChat public key. Check your internet connection.${NC}"
        exit 1
    fi
    
    echo -e "${GREEN}✓ Successfully retrieved OpenChat public key${NC}"
    echo ""
}

# Function to get Mistral API key
get_mistral_key() {
    echo -e "${BOLD}Setting up Mistral AI API key...${NC}"
    
    # Check if API key already exists in .env
    if [[ -f "${SRC_DIR}/${ENV_FILE}" ]] && grep -q "MISTRAL_API_KEY" "${SRC_DIR}/${ENV_FILE}"; then
        CURRENT_KEY=$(grep "MISTRAL_API_KEY" "${SRC_DIR}/${ENV_FILE}" | cut -d'=' -f2)
        if [[ ! -z "$CURRENT_KEY" && "$CURRENT_KEY" != "your-mistral-api-key-here" ]]; then
            echo -e "${YELLOW}⚠️ Mistral API key already found in ${ENV_FILE}${NC}"
            read -p "Do you want to use the existing key? (y/n): " use_existing_key
            if [[ $use_existing_key == "y" || $use_existing_key == "Y" ]]; then
                MISTRAL_API_KEY="$CURRENT_KEY"
                echo "Using existing Mistral API key."
                return
            fi
        fi
    fi
    
    # Prompt for API key
    read -p "Enter your Mistral AI API key: " MISTRAL_API_KEY
    
    # Validate key is not empty
    while [[ -z "$MISTRAL_API_KEY" ]]; do
        echo -e "${RED}API key cannot be empty.${NC}"
        read -p "Enter your Mistral AI API key: " MISTRAL_API_KEY
    done
    
    echo -e "${GREEN}✓ Mistral API key configured${NC}"
    echo ""
}

# Function to create configuration files
create_config_files() {
    echo -e "${BOLD}Creating configuration files...${NC}"
    
    # Create config.toml
    echo "Creating ${SRC_DIR}/${CONFIG_FILE}..."
    cat > "${SRC_DIR}/${CONFIG_FILE}" << EOF
pem_file = "./identity.pem"
ic_url = "https://icp0.io"
port = $PORT
oc_public_key = """
$OC_PUBLIC_KEY
"""
log_level = "INFO"
mistral_api_key = "$MISTRAL_API_KEY" # Also configurable via environment variable
sqlite_db_path = "$DB_PATH"

[agent]
enable_agent_planning = true
enable_memory = true
enable_summarization = true
enable_moderation = true
memory_retention_days = 30
max_memory_items = 1000
EOF
    
    # Create .env file
    echo "Creating ${SRC_DIR}/${ENV_FILE}..."
    cat > "${SRC_DIR}/${ENV_FILE}" << EOF
# KarmaSpark Environment Configuration

# Mistral AI API Key (required for LLM functionality)
MISTRAL_API_KEY=$MISTRAL_API_KEY

# Optional: Path to config file (defaults to ./config.toml)
# CONFIG_FILE=./config.toml

# Optional: Override port from config file
# PORT=$PORT
EOF
    
    # Create .env.example file
    echo "Creating ${SRC_DIR}/${ENV_EXAMPLE_FILE}..."
    cat > "${SRC_DIR}/${ENV_EXAMPLE_FILE}" << EOF
# KarmaSpark Environment Configuration

# Mistral AI API Key (required for LLM functionality)
MISTRAL_API_KEY=your-mistral-api-key-here

# Optional: Path to config file (defaults to ./config.toml)
# CONFIG_FILE=./config.toml

# Optional: Override port from config file
# PORT=$PORT
EOF
    
    echo -e "${GREEN}✓ Configuration files created successfully${NC}"
    echo ""
}

# Function to build and test the bot
build_bot() {
    echo -e "${BOLD}Building KarmaSpark bot...${NC}"
    
    # Change to the src directory if we're not already there
    if [[ "$SRC_DIR" != "." ]]; then
        cd "$SRC_DIR"
    fi
    
    # Build the bot
    cargo build
    
    if [ $? -eq 0 ]; then
        echo -e "${GREEN}✓ Build successful${NC}"
    else
        echo -e "${RED}❌ Build failed${NC}"
        exit 1
    fi
    
    # Return to original directory
    if [[ "$SRC_DIR" != "." ]]; then
        cd ..
    fi
    
    echo ""
}

# Function to display usage instructions
show_instructions() {
    echo -e "${BOLD}${BLUE}╔═══════════════════════════════════════════════════════════╗${NC}"
    echo -e "${BOLD}${BLUE}║                  SETUP COMPLETE!                          ║${NC}"
    echo -e "${BOLD}${BLUE}╚═══════════════════════════════════════════════════════════╝${NC}"
    echo ""
    echo -e "${BOLD}To run your bot:${NC}"
    echo -e "  ${GREEN}cd ${SRC_DIR} && cargo run${NC}"
    echo ""
    echo -e "${BOLD}To register your bot with OpenChat:${NC}"
    echo "  1. Start your bot: cd ${SRC_DIR} && cargo run"
    echo "  2. Make sure it's accessible from the internet (may require port forwarding)"
    echo "  3. Visit: https://oc.app/botregistry"
    echo "  4. Use your bot's endpoint URL (http://YOUR_IP:$PORT)"
    echo ""
    echo -e "${BOLD}Bot capabilities:${NC}"
    echo "  • /ask - Ask KarmaSpark a question"
    echo "  • /summarize - Summarize long text"
    echo "  • /memory - Store or recall information"
    echo "  • /moderate - Check content for harmful material"
    echo "  • /remindme - Set reminders"
    echo "  • /echo - Simple echo command for testing"
    echo ""
    echo -e "${YELLOW}Note: Your bot needs to be running and publicly accessible to be registered.${NC}"
    echo ""
}

# Main execution flow
check_prerequisites
setup_identity
fetch_openchat_key
get_mistral_key
create_config_files
build_bot
show_instructions

exit 0 