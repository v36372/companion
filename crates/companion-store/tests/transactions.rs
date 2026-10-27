use companion_store::SqliteHub;
use std::{
    sync::{Arc, mpsc},
    time::Duration,
};

#[test]
fn app_writer_and_other_store_wait_until_guest_commit() {
    let dir = tempfile::tempdir().unwrap();
    let hub = Arc::new(SqliteHub::open(dir.path().join("companion.sqlite")).unwrap());
    hub.write(|c| {
        c.execute_batch("CREATE TABLE probe(value INTEGER)")?;
        Ok(())
    })
    .unwrap();
    hub.begin(1).unwrap();
    hub.guest(1, |c| {
        c.execute("INSERT INTO probe VALUES(1)", [])?;
        Ok(())
    })
    .unwrap();
    let reader = hub.open_read_only().unwrap();
    assert_eq!(
        reader
            .query_row("SELECT count(*) FROM probe", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    let (ready, started) = mpsc::channel();
    let (done, completed) = mpsc::channel();
    let app = hub.clone();
    let app_ready = ready.clone();
    let app_done = done.clone();
    let writer = std::thread::spawn(move || {
        app_ready.send(()).unwrap();
        app.write(|c| {
            c.execute("INSERT INTO probe VALUES(2)", [])?;
            Ok(())
        })
        .unwrap();
        app_done.send(()).unwrap();
    });
    let other = hub.clone();
    let guest = std::thread::spawn(move || {
        ready.send(()).unwrap();
        other
            .guest(2, |c| {
                c.execute("INSERT INTO probe VALUES(3)", [])?;
                Ok(())
            })
            .unwrap();
        done.send(()).unwrap();
    });
    started.recv().unwrap();
    started.recv().unwrap();
    assert!(completed.recv_timeout(Duration::from_millis(100)).is_err());
    hub.end(1, true).unwrap();
    completed.recv_timeout(Duration::from_secs(2)).unwrap();
    completed.recv_timeout(Duration::from_secs(2)).unwrap();
    writer.join().unwrap();
    guest.join().unwrap();
    assert_eq!(
        reader
            .query_row("SELECT count(*) FROM probe", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        3
    );
}

#[test]
fn abandoned_store_rolls_back_and_wakes_writers() {
    let hub = Arc::new(SqliteHub::open(":memory:").unwrap());
    hub.begin(7).unwrap();
    hub.guest(7, |c| {
        c.execute_batch("CREATE TABLE abandoned(value INTEGER)")?;
        Ok(())
    })
    .unwrap();
    assert!(hub.end(8, true).is_err());
    let (done, completed) = mpsc::channel();
    let other = hub.clone();
    let writer = std::thread::spawn(move || {
        other
            .write(|c| {
                let exists: bool = c.query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='abandoned')",
                    [],
                    |r| r.get(0),
                )?;
                assert!(!exists);
                Ok(())
            })
            .unwrap();
        done.send(()).unwrap();
    });
    assert!(completed.recv_timeout(Duration::from_millis(100)).is_err());
    hub.release(7);
    completed.recv_timeout(Duration::from_secs(2)).unwrap();
    writer.join().unwrap();
}
