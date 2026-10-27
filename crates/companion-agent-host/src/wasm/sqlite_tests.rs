use super::{
    AgentHostState,
    companion::agent::{sqlite::Host, types::SqliteValue},
};
use crate::http_proxy::{Abort, Policy};
use std::sync::Arc;

#[test]
fn sqlite_values_and_store_drop_rollback() {
    let hub = Arc::new(companion_store::SqliteHub::open(":memory:").unwrap());
    let (events, _) = crossbeam_channel::unbounded();
    let mut state = AgentHostState::new(Abort::default(), Policy::default(), events, hub.clone());
    state
        .exec("CREATE TABLE probe(n, r, i, big, t, b)".into())
        .unwrap();
    let values = vec![
        SqliteValue::Null,
        SqliteValue::Real(1.25),
        SqliteValue::Integer(42),
        SqliteValue::Integer(9_007_199_254_740_993),
        SqliteValue::Text("🌻".into()),
        SqliteValue::Blob(vec![0, 255, 128]),
    ];
    state
        .run("INSERT INTO probe VALUES(?, ?, ?, ?, ?, ?)".into(), values)
        .unwrap();
    let json = state
        .get("SELECT * FROM probe".into(), vec![])
        .unwrap()
        .unwrap();
    let row: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(row["n"]["tag"], "null");
    assert_eq!(row["r"]["val"], 1.25);
    assert_eq!(row["i"]["val"], "42");
    assert_eq!(row["big"]["val"], "9007199254740993");
    assert_eq!(row["t"]["val"], "🌻");
    assert_eq!(row["b"]["val"], serde_json::json!([0, 255, 128]));
    assert!(
        state
            .get("SELECT * FROM probe WHERE 0".into(), vec![])
            .unwrap()
            .is_none()
    );
    let all: Vec<serde_json::Value> =
        serde_json::from_str(&state.all("SELECT * FROM probe".into(), vec![]).unwrap()).unwrap();
    assert_eq!(all, vec![row]);
    state.begin().unwrap();
    state.exec("DELETE FROM probe".into()).unwrap();
    drop(state);
    hub.write(|c| {
        assert_eq!(
            c.query_row("SELECT count(*) FROM probe", [], |r| r.get::<_, i64>(0))?,
            1
        );
        Ok(())
    })
    .unwrap();
}
