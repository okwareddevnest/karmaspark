use anyhow::{anyhow, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub id: Option<i64>,
    pub chat_id: String,
    pub user_id: String,
    pub timestamp: DateTime<Utc>,
    pub content: String,
    pub embedding: Option<Vec<f32>>,
    pub metadata: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MemoryStore {
    db: Arc<Mutex<Connection>>,
}

#[async_trait]
pub trait EmbeddingModel {
    async fn embed_text(&self, text: &str) -> Result<Vec<f32>>;
    async fn similarity(&self, embedding1: &[f32], embedding2: &[f32]) -> f32;
}

impl MemoryStore {
    pub fn new<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        
        // Create tables if they don't exist
        conn.execute(
            "CREATE TABLE IF NOT EXISTS memories (
                id INTEGER PRIMARY KEY,
                chat_id TEXT NOT NULL,
                user_id TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                content TEXT NOT NULL,
                embedding BLOB,
                metadata TEXT,
                UNIQUE(chat_id, user_id, timestamp)
            )",
            [],
        )?;
        
        conn.execute(
            "CREATE INDEX IF NOT EXISTS memories_chat_id_idx ON memories (chat_id)",
            [],
        )?;
        
        Ok(Self {
            db: Arc::new(Mutex::new(conn)),
        })
    }
    
    pub async fn store_memory(&self, memory: Memory) -> Result<i64> {
        let db = self.db.clone();
        
        let result = tokio::task::spawn_blocking(move || -> Result<i64> {
            let conn = db.lock().unwrap();
            
            let embedding_blob = memory.embedding.as_ref().map(|e| {
                let bytes: Vec<u8> = e.iter()
                    .flat_map(|&f| f.to_le_bytes().to_vec())
                    .collect();
                bytes
            });
            
            conn.execute(
                "INSERT OR REPLACE INTO memories 
                (chat_id, user_id, timestamp, content, embedding, metadata) 
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    memory.chat_id,
                    memory.user_id,
                    memory.timestamp.to_rfc3339(),
                    memory.content,
                    embedding_blob,
                    memory.metadata,
                ],
            )?;
            
            Ok(conn.last_insert_rowid())
        }).await??;
        
        Ok(result)
    }
    
    pub async fn get_recent_memories(&self, chat_id: &str, limit: usize) -> Result<Vec<Memory>> {
        let chat_id = chat_id.to_string();
        let db = self.db.clone();
        
        let memories = tokio::task::spawn_blocking(move || -> Result<Vec<Memory>> {
            let conn = db.lock().unwrap();
            
            let mut stmt = conn.prepare(
                "SELECT id, chat_id, user_id, timestamp, content, embedding, metadata 
                 FROM memories 
                 WHERE chat_id = ?1 
                 ORDER BY timestamp DESC 
                 LIMIT ?2"
            )?;
            
            let rows = stmt.query_map(params![chat_id, limit as i64], |row| {
                let id = row.get(0)?;
                let chat_id = row.get(1)?;
                let user_id = row.get(2)?;
                let timestamp_str: String = row.get(3)?;
                let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());
                let content = row.get(4)?;
                let embedding_blob: Option<Vec<u8>> = row.get(5)?;
                let embedding = embedding_blob.map(|blob| {
                    let mut embedding = Vec::new();
                    for chunk in blob.chunks(4) {
                        if chunk.len() == 4 {
                            let mut bytes = [0u8; 4];
                            bytes.copy_from_slice(chunk);
                            embedding.push(f32::from_le_bytes(bytes));
                        }
                    }
                    embedding
                });
                let metadata = row.get(6)?;
                
                Ok(Memory {
                    id: Some(id),
                    chat_id,
                    user_id,
                    timestamp,
                    content,
                    embedding,
                    metadata,
                })
            })?;
            
            let mut memories = Vec::new();
            for row in rows {
                memories.push(row?);
            }
            
            Ok(memories)
        }).await??;
        
        Ok(memories)
    }
    
    pub async fn search_similar_memories(
        &self, 
        chat_id: &str, 
        query_embedding: &[f32], 
        limit: usize
    ) -> Result<Vec<(Memory, f32)>> {
        let chat_id = chat_id.to_string();
        let query_embedding = query_embedding.to_vec();
        let db = self.db.clone();
        
        let memories = tokio::task::spawn_blocking(move || -> Result<Vec<(Memory, f32)>> {
            let conn = db.lock().unwrap();
            
            let mut memories_with_score = Vec::new();
            let mut stmt = conn.prepare(
                "SELECT id, chat_id, user_id, timestamp, content, embedding, metadata 
                 FROM memories 
                 WHERE chat_id = ?1 AND embedding IS NOT NULL"
            )?;
            
            let rows = stmt.query_map(params![chat_id], |row| {
                let id = row.get(0)?;
                let chat_id = row.get(1)?;
                let user_id = row.get(2)?;
                let timestamp_str: String = row.get(3)?;
                let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());
                let content = row.get(4)?;
                let embedding_blob: Option<Vec<u8>> = row.get(5)?;
                let embedding = embedding_blob.map(|blob| {
                    let mut embedding = Vec::new();
                    for chunk in blob.chunks(4) {
                        if chunk.len() == 4 {
                            let mut bytes = [0u8; 4];
                            bytes.copy_from_slice(chunk);
                            embedding.push(f32::from_le_bytes(bytes));
                        }
                    }
                    embedding
                });
                let metadata = row.get(6)?;
                
                Ok(Memory {
                    id: Some(id),
                    chat_id,
                    user_id,
                    timestamp,
                    content,
                    embedding,
                    metadata,
                })
            })?;
            
            for row in rows {
                let memory = row?;
                if let Some(ref embedding) = memory.embedding {
                    // Calculate cosine similarity
                    let similarity = cosine_similarity(&query_embedding, embedding);
                    memories_with_score.push((memory, similarity));
                }
            }
            
            // Sort by similarity score
            memories_with_score.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            
            // Return top N results
            Ok(memories_with_score.into_iter().take(limit).collect())
        }).await??;
        
        Ok(memories)
    }
    
    pub async fn cleanup_old_memories(&self, chat_id: &str, days_to_keep: u32) -> Result<usize> {
        let chat_id = chat_id.to_string();
        let db = self.db.clone();
        
        let deleted = tokio::task::spawn_blocking(move || -> Result<usize> {
            let conn = db.lock().unwrap();
            
            let cutoff_date = (Utc::now() - chrono::Duration::days(days_to_keep as i64)).to_rfc3339();
            
            let deleted = conn.execute(
                "DELETE FROM memories WHERE chat_id = ?1 AND timestamp < ?2",
                params![chat_id, cutoff_date],
            )?;
            
            Ok(deleted)
        }).await??;
        
        Ok(deleted)
    }

    /// Get memory by ID
    pub async fn get_memory(&self, id: i64) -> Result<Option<Memory>> {
        let db = self.db.clone();
        
        tokio::task::spawn_blocking(move || {
            let conn = db.lock().unwrap();
            
            let result = conn.query_row(
                "SELECT id, chat_id, user_id, timestamp, content, embedding, metadata 
                 FROM memories WHERE id = ?1",
                params![id],
                |row| {
                    let id = row.get(0)?;
                    let chat_id = row.get(1)?;
                    let user_id = row.get(2)?;
                    let timestamp_str: String = row.get(3)?;
                    let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now());
                    let content = row.get(4)?;
                    let embedding_blob: Option<Vec<u8>> = row.get(5)?;
                    let embedding = embedding_blob.map(|blob| {
                        let mut embedding = Vec::new();
                        for chunk in blob.chunks(4) {
                            if chunk.len() == 4 {
                                let mut bytes = [0u8; 4];
                                bytes.copy_from_slice(chunk);
                                embedding.push(f32::from_le_bytes(bytes));
                            }
                        }
                        embedding
                    });
                    let metadata = row.get(6)?;
                    
                    Ok(Memory {
                        id: Some(id),
                        chat_id,
                        user_id,
                        timestamp,
                        content,
                        embedding,
                        metadata,
                    })
                },
            );
            
            match result {
                Ok(memory) => Ok(Some(memory)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(anyhow!("Error retrieving memory: {}", e)),
            }
        }).await?
    }
}

// Utility function to calculate cosine similarity between two vectors
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    
    let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let magnitude_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let magnitude_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    
    if magnitude_a == 0.0 || magnitude_b == 0.0 {
        return 0.0;
    }
    
    dot_product / (magnitude_a * magnitude_b)
} 