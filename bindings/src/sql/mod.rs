use std::sync::Arc;
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use crate::BrowserDB;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SqlType {
    Integer,
    Text,
    Boolean,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnDef {
    pub name: String,
    pub col_type: SqlType,
    pub is_primary_key: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableSchema {
    pub name: String,
    pub columns: Vec<ColumnDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SqlValue {
    Integer(i64),
    Text(String),
    Boolean(bool),
    Null,
}

impl std::fmt::Display for SqlValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SqlValue::Integer(v) => write!(f, "{}", v),
            SqlValue::Text(v) => write!(f, "'{}'", v),
            SqlValue::Boolean(v) => write!(f, "{}", v),
            SqlValue::Null => write!(f, "NULL"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SqlRow {
    pub values: HashMap<String, SqlValue>,
}

pub struct SqlEngine {
    db: Arc<BrowserDB>,
}

impl SqlEngine {
    pub fn new(db: Arc<BrowserDB>) -> Self {
        Self { db }
    }

    pub fn execute(&self, query: &str) -> Result<String, Box<dyn std::error::Error>> {
        let tokens: Vec<&str> = query.split_whitespace().collect();
        if tokens.is_empty() {
            return Ok("Empty query".to_string());
        }

        match tokens[0].to_uppercase().as_str() {
            "CREATE" => self.handle_create(query),
            "INSERT" => self.handle_insert(query),
            "SELECT" => self.handle_select(query),
            _ => Err(format!("Unsupported command: {}", tokens[0]).into()),
        }
    }

    // Syntax: CREATE TABLE table_name (col1 TYPE, col2 TYPE, id TYPE PRIMARY KEY)
    // Simplified: CREATE TABLE users (id INT PRIMARY_KEY, name TEXT)
    fn handle_create(&self, query: &str) -> Result<String, Box<dyn std::error::Error>> {
        let start_paren = query.find('(').ok_or("Missing '('")?;
        let end_paren = query.find(')').ok_or("Missing ')'")?;
        let table_name = query[13..start_paren].trim();
        let columns_str = &query[start_paren+1..end_paren];
        
        let mut columns = Vec::new();
        for col_def in columns_str.split(',') {
            let parts: Vec<&str> = col_def.trim().split_whitespace().collect();
            if parts.len() < 2 { continue; } 
            
            let name = parts[0].to_string();
            let type_str = parts[1].to_uppercase();
            let is_pk = parts.len() > 2 && parts[2].to_uppercase() == "PRIMARY_KEY";
            
            let col_type = match type_str.as_str() {
                "INT" | "INTEGER" => SqlType::Integer,
                "TEXT" | "STRING" => SqlType::Text,
                "BOOL" | "BOOLEAN" => SqlType::Boolean,
                _ => return Err(format!("Unknown type: {}", type_str).into()),
            };
            
            columns.push(ColumnDef { name, col_type, is_primary_key: is_pk });
        }

        let schema = TableSchema {
            name: table_name.to_string(),
            columns,
        };

        self.save_schema(&schema)?;
        Ok(format!("Table '{}' created.", table_name))
    }

    // Syntax: INSERT INTO table VALUES (val1, val2)
    fn handle_insert(&self, query: &str) -> Result<String, Box<dyn std::error::Error>> {
        let start_paren = query.find('(').ok_or("Missing values list")?;
        let end_paren = query.find(')').ok_or("Missing closing ')'")?;
        
        let pre_vals = &query[..start_paren];
        let parts: Vec<&str> = pre_vals.split_whitespace().collect();
        // INSERT INTO table VALUES
        if parts.len() < 4 { return Err("Invalid INSERT syntax".into()); } 
        let table_name = parts[2];

        let schema = self.get_schema(table_name)?.ok_or("Table not found")?;
        
        let val_str = &query[start_paren+1..end_paren];
        let raw_values: Vec<&str> = val_str.split(',').map(|s| s.trim()).collect();

        if raw_values.len() != schema.columns.len() {
            return Err("Column count mismatch".into());
        }

        let mut row_data = HashMap::new();
        let mut pk_value = String::new();

        for (i, col) in schema.columns.iter().enumerate() {
            let raw = raw_values[i];
            let val = match col.col_type {
                SqlType::Integer => SqlValue::Integer(raw.parse()?),
                SqlType::Text => SqlValue::Text(raw.trim_matches('\'').to_string()),
                SqlType::Boolean => SqlValue::Boolean(raw.parse()?),
            };
            
            if col.is_primary_key {
                pk_value = raw.to_string();
            }
            
            row_data.insert(col.name.clone(), val);
        }

        if pk_value.is_empty() {
            return Err("Primary key required".into());
        }

        let key = format!("sql:data:{}:{}", table_name, pk_value);
        let value = bincode::serialize(&row_data)?;
        
        self.db.put_raw_localstore(key.into_bytes(), value)?;
        
        Ok(format!("Inserted row with PK {}", pk_value))
    }

    // Syntax: SELECT * FROM table WHERE id = val
    fn handle_select(&self, query: &str) -> Result<String, Box<dyn std::error::Error>> {
        let parts: Vec<&str> = query.split_whitespace().collect();
        if parts.len() < 8 { return Err("Only 'SELECT * FROM table WHERE id = val' supported".into()); }
        
        let table_name = parts[3];
        let _col_name = parts[5];
        let val = parts[7];
        
        // Validate schema existence
        let _schema = self.get_schema(table_name)?.ok_or("Table not found")?;
        
        let key = format!("sql:data:{}:{}", table_name, val);
        
        if let Some(data) = self.db.get_raw_localstore(key.as_bytes())? {
             let row: HashMap<String, SqlValue> = bincode::deserialize(&data)?;
             let mut output = String::new();
             for (k, v) in row {
                 output.push_str(&format!("{}: {}, ", k, v));
             }
             Ok(output)
        } else {
            Ok("No result found".to_string())
        }
    }

    fn save_schema(&self, schema: &TableSchema) -> Result<(), Box<dyn std::error::Error>> {
        let key = format!("sql:schema:{}", schema.name);
        let value = bincode::serialize(schema)?;
        self.db.put_raw_localstore(key.into_bytes(), value)?;
        Ok(())
    }

    fn get_schema(&self, name: &str) -> Result<Option<TableSchema>, Box<dyn std::error::Error>> {
        let key = format!("sql:schema:{}", name);
        if let Some(data) = self.db.get_raw_localstore(key.as_bytes())? {
            let schema = bincode::deserialize(&data)?;
            Ok(Some(schema))
        } else {
            Ok(None)
        }
    }
}
