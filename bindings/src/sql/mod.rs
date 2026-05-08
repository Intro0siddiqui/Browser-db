use std::sync::Arc;
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use crate::BrowserDB;
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;
use sqlparser::ast::{Statement, DataType, ColumnOption, Expr, Value};

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

pub struct SqlEngine {
    db: Arc<BrowserDB>,
}

impl SqlEngine {
    pub fn new(db: Arc<BrowserDB>) -> Self {
        Self { db }
    }

    pub fn execute(&self, query: &str) -> Result<String, Box<dyn std::error::Error>> {
        let dialect = GenericDialect {};
        let ast = Parser::parse_sql(&dialect, query)?;

        if ast.is_empty() {
            return Ok("Empty query".to_string());
        }

        match &ast[0] {
            Statement::CreateTable { name, columns, .. } => self.handle_create(name.to_string(), columns),
            Statement::Insert { table_name, source, .. } => self.handle_insert(table_name.to_string(), source),
            Statement::Query(query_ast) => self.handle_select(query_ast),
            _ => Err(format!("Unsupported statement: {:?}", ast[0]).into()),
        }
    }

    fn handle_create(&self, table_name: String, columns: &[sqlparser::ast::ColumnDef]) -> Result<String, Box<dyn std::error::Error>> {
        let mut col_defs = Vec::new();
        for col in columns {
            let col_type = match &col.data_type {
                DataType::Integer(_) | DataType::Int(_) | DataType::BigInt(_) => SqlType::Integer,
                DataType::Text | DataType::Varchar(_) | DataType::String(_) => SqlType::Text,
                DataType::Boolean => SqlType::Boolean,
                _ => return Err(format!("Unsupported type: {:?}", col.data_type).into()),
            };
            
            let is_pk = col.options.iter().any(|opt| {
                match &opt.option {
                    ColumnOption::Unique { is_primary, .. } => *is_primary,
                    _ => false,
                }
            });

            col_defs.push(ColumnDef {
                name: col.name.value.clone(),
                col_type,
                is_primary_key: is_pk,
            });
        }

        let schema = TableSchema {
            name: table_name.clone(),
            columns: col_defs,
        };

        self.save_schema(&schema)?;
        Ok(format!("Table '{}' created.", table_name))
    }

    fn handle_insert(&self, table_name: String, source: &Option<Box<sqlparser::ast::Query>>) -> Result<String, Box<dyn std::error::Error>> {
        let schema = self.get_schema(&table_name)?.ok_or_else(|| format!("Table '{}' not found", table_name))?;
        
        let query = source.as_ref().ok_or("No values provided for INSERT")?;
        let body = &query.body;
        
        if let sqlparser::ast::SetExpr::Values(values) = &**body {
            let mut inserted_count = 0;
            for row in &values.rows {
                if row.len() != schema.columns.len() {
                    return Err("Column count mismatch".into());
                }

                let mut row_data = HashMap::new();
                let mut pk_value = String::new();

                for (i, col) in schema.columns.iter().enumerate() {
                    let val = match &row[i] {
                        Expr::Value(Value::Number(n, _)) => SqlValue::Integer(n.parse()?),
                        Expr::Value(Value::SingleQuotedString(s)) => SqlValue::Text(s.clone()),
                        Expr::Value(Value::Boolean(b)) => SqlValue::Boolean(*b),
                        _ => return Err(format!("Unsupported value: {:?}", row[i]).into()),
                    };

                    if col.is_primary_key {
                        pk_value = format!("{}", val);
                    }
                    row_data.insert(col.name.clone(), val);
                }

                if pk_value.is_empty() {
                    return Err("Primary key required".into());
                }

                let key = format!("sql:data:{}:{}", table_name, pk_value);
                let value = bincode::serialize(&row_data)?;
                self.db.put_raw_localstore(key.into_bytes(), value)?;
                inserted_count += 1;
            }
            Ok(format!("Inserted {} rows into '{}'", inserted_count, table_name))
        } else {
            Err("Unsupported INSERT source".into())
        }
    }

    fn handle_select(&self, query: &sqlparser::ast::Query) -> Result<String, Box<dyn std::error::Error>> {
        if let sqlparser::ast::SetExpr::Select(select) = &*query.body {
            let from = &select.from;
            if from.is_empty() { return Err("Missing FROM clause".into()); }
            let table_name = from[0].relation.to_string();

            let schema = self.get_schema(&table_name)?.ok_or_else(|| format!("Table '{}' not found", table_name))?;

            // Simplified: check for WHERE id = val
            if let Some(selection) = &select.selection {
                if let Expr::BinaryOp { left, op, right } = selection {
                    if matches!(op, sqlparser::ast::BinaryOperator::Eq) {
                        let col_name = left.to_string();
                        let val = match &**right {
                            Expr::Value(Value::Number(n, _)) => n.clone(),
                            Expr::Value(Value::SingleQuotedString(s)) => s.clone(),
                            _ => right.to_string(),
                        };

                        let pk_col = schema.columns.iter().find(|c| c.is_primary_key).ok_or("Table has no primary key")?;
                        if col_name == pk_col.name {
                            let key = format!("sql:data:{}:{}", table_name, val);
                            if let Some(data) = self.db.get_raw_localstore(key.as_bytes())? {
                                let row: HashMap<String, SqlValue> = bincode::deserialize(&data)?;
                                let mut output = String::new();
                                for (k, v) in row {
                                    output.push_str(&format!("{}: {}, ", k, v));
                                }
                                return Ok(output);
                            }
                        }
                    }
                }
            }
            Ok("No results found".to_string())
        } else {
            Err("Unsupported SELECT query".into())
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
