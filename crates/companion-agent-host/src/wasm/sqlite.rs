//! Typed WIT parameters; tagged JSON column values preserve blobs and s64 in
//! the string row transport without confusing user text with encoded values.
use super::{
    AgentHostState,
    companion::agent::{
        sqlite::Host,
        types::{SqliteError, SqliteValue},
    },
};
use rusqlite::{
    params_from_iter,
    types::{Value, ValueRef},
};
use serde_json::{Value as Json, json};

fn error(error: anyhow::Error) -> SqliteError {
    SqliteError {
        message: error.to_string(),
    }
}
fn params(values: Vec<SqliteValue>) -> Vec<Value> {
    values
        .into_iter()
        .map(|value| match value {
            SqliteValue::Null => Value::Null,
            SqliteValue::Real(v) => Value::Real(v),
            SqliteValue::Integer(v) => Value::Integer(v),
            SqliteValue::Text(v) => Value::Text(v),
            SqliteValue::Blob(v) => Value::Blob(v),
        })
        .collect()
}
fn query(
    state: &AgentHostState,
    sql: &str,
    values: Vec<SqliteValue>,
) -> Result<Vec<Json>, SqliteError> {
    state
        .sqlite
        .guest(state.sqlite_owner, |connection| {
            let mut statement = connection.prepare(sql)?;
            let names = statement
                .column_names()
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>();
            let values = params(values);
            let mut rows = statement.query(params_from_iter(values))?;
            let mut result = Vec::new();
            while let Some(row) = rows.next()? {
                let mut object = serde_json::Map::new();
                for (i, name) in names.iter().enumerate() {
                    let value = match row.get_ref(i)? {
                        ValueRef::Null => json!({"tag":"null"}),
                        ValueRef::Real(v) => json!({"tag":"real", "val":v}),
                        ValueRef::Integer(v) => json!({"tag":"integer", "val":v.to_string()}),
                        ValueRef::Text(v) => json!({"tag":"text", "val":std::str::from_utf8(v)?}),
                        ValueRef::Blob(v) => json!({"tag":"blob", "val":v}),
                    };
                    object.insert(name.clone(), value);
                }
                result.push(Json::Object(object));
            }
            Ok(result)
        })
        .map_err(error)
}
impl Host for AgentHostState {
    fn exec(&mut self, sql: String) -> Result<(), SqliteError> {
        self.sqlite
            .guest(self.sqlite_owner, |c| {
                c.execute_batch(&sql)?;
                Ok(())
            })
            .map_err(error)
    }
    fn run(&mut self, sql: String, values: Vec<SqliteValue>) -> Result<(), SqliteError> {
        self.sqlite
            .guest(self.sqlite_owner, |c| {
                c.execute(&sql, params_from_iter(params(values)))?;
                Ok(())
            })
            .map_err(error)
    }
    fn get(
        &mut self,
        sql: String,
        values: Vec<SqliteValue>,
    ) -> Result<Option<String>, SqliteError> {
        Ok(query(self, &sql, values)?
            .into_iter()
            .next()
            .map(|row| row.to_string()))
    }
    fn all(&mut self, sql: String, values: Vec<SqliteValue>) -> Result<String, SqliteError> {
        Ok(Json::Array(query(self, &sql, values)?).to_string())
    }
    fn begin(&mut self) -> Result<(), SqliteError> {
        self.sqlite.begin(self.sqlite_owner).map_err(error)
    }
    fn commit(&mut self) -> Result<(), SqliteError> {
        self.sqlite.end(self.sqlite_owner, true).map_err(error)
    }
    fn rollback(&mut self) -> Result<(), SqliteError> {
        self.sqlite.end(self.sqlite_owner, false).map_err(error)
    }
}
