use super::*;
use crate::wasm::Event;
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::{Duration, Instant},
};

fn server(delay_headers: bool, delay_body: bool) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket.set_nodelay(true).unwrap();
        let mut request = [0; 4096];
        let _ = socket.read(&mut request);
        if delay_headers {
            std::thread::sleep(Duration::from_secs(5));
        }
        if socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").is_err() { return; }
        if delay_body {
            std::thread::sleep(Duration::from_secs(5));
        }
        for i in 0..30 {
            let data = format!("data: {{\"sequence\":{i}}}\n\n");
            let chunk = format!("{:x}\r\n{}\r\n", data.len(), data);
            if socket.write_all(chunk.as_bytes()).is_err() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = socket.write_all(b"0\r\n\r\n");
    });
    port
}

fn run_case(
    runtime: &wasm::Runtime,
    name: &str,
    abort_chunks: Option<usize>,
    delayed_headers: bool,
    delayed_body: bool,
) {
    let port = server(delayed_headers, delayed_body);
    let abort = Abort::default();
    let (tx, rx) = crossbeam_channel::unbounded();
    let (events, received) = crossbeam_channel::unbounded();
    let thread = spawn(
        runtime.clone(),
        abort.clone(),
        Policy {
            loopback_port: Some(port),
        },
        rx,
        events,
    );
    match received.recv_timeout(Duration::from_secs(300)).unwrap() {
        Event::Ready {
            cold_start_ms,
            compile_ms,
        } => println!(
            "COLD_START {name} instantiate={cold_start_ms:.3} ms compile={compile_ms:.3} ms"
        ),
        e => panic!("expected ready: {e:?}"),
    }
    let start = Instant::now();
    tx.send(Command::Sse(format!("http://127.0.0.1:{port}/sse")))
        .unwrap();
    let timer = if delayed_headers || delayed_body {
        let abort = abort.clone();
        Some(std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            abort.request();
            Instant::now()
        }))
    } else {
        None
    };
    let mut stamps = Vec::new();
    let mut aborted_at = None;
    let result = loop {
        match received
            .recv_timeout(Duration::from_secs(10))
            .expect("export must return promptly")
        {
            Event::Chunk { at, json } => {
                let value: serde_json::Value = serde_json::from_str(&json).unwrap();
                assert_eq!(value["sequence"].as_u64().unwrap(), stamps.len() as u64);
                let ms = at.duration_since(start).as_secs_f64() * 1000.;
                println!("CHUNK {name} {} {ms:.3} ms", stamps.len());
                stamps.push(at);
                if abort_chunks == Some(stamps.len()) {
                    aborted_at = Some(Instant::now());
                    abort.request();
                }
            }
            Event::Done(result) => break result,
            e => panic!("unexpected: {e:?}"),
        }
    };
    let elapsed = start.elapsed();
    if let Some(timer) = timer {
        aborted_at = Some(timer.join().unwrap());
    }
    println!(
        "DONE {name} {:.3} ms result={result:?}",
        elapsed.as_secs_f64() * 1000.
    );
    if let Some(at) = aborted_at {
        assert!(result.is_err(), "abort must fail fetch/body");
        let latency = at.elapsed();
        println!("ABORT {name} {:.3} ms", latency.as_secs_f64() * 1000.);
        assert!(latency < Duration::from_secs(1));
        if delayed_headers || delayed_body {
            assert!(elapsed < Duration::from_secs(1));
        }
    } else {
        assert!(result.is_ok());
        assert!(stamps.len() >= 10, "must see incremental chunks");
        assert!(stamps[0].duration_since(start) < Duration::from_secs(1));
        assert!(stamps.last().unwrap().duration_since(stamps[0]) > Duration::from_secs(2));
        let good_gaps = stamps
            .windows(2)
            .filter(|w| {
                let gap = w[1].duration_since(w[0]);
                gap > Duration::from_millis(50) && gap < Duration::from_millis(250)
            })
            .count();
        assert!(good_gaps >= 10, "chunks must arrive ~100ms apart");
    }
    let _ = tx.send(Command::Close);
    thread.join().unwrap().unwrap();
}

#[test]
fn spike_sse() {
    let runtime = wasm::Runtime::load().unwrap();
    run_case(&runtime, "incremental", None, false, false);
    run_case(&runtime, "mid-body", Some(5), false, false);
    run_case(&runtime, "ttfb", None, true, false);
    run_case(&runtime, "headers-no-body", None, false, true);

    let engine = runtime.engine.clone();
    let abort = Abort::default();
    let (tx, rx) = crossbeam_channel::unbounded();
    let (events, received) = crossbeam_channel::unbounded();
    let thread = spawn(runtime, abort.clone(), Policy::default(), rx, events);
    assert!(matches!(
        received.recv_timeout(Duration::from_secs(300)).unwrap(),
        Event::Ready { .. }
    ));
    tx.send(Command::Busy).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let at = Instant::now();
    abort.request();
    engine.increment_epoch();
    match received.recv_timeout(Duration::from_secs(1)).unwrap() {
        Event::Done(Err(error)) => {
            println!("EPOCH {:.3} ms {error}", at.elapsed().as_secs_f64() * 1000.);
            assert!(error.contains("interrupt"));
        }
        e => panic!("expected epoch trap: {e:?}"),
    }
    thread.join().unwrap().unwrap();
    println!("VERDICT: PASS (on-stack incremental fetch, mid-body/TTFB abort, epoch busy-loop)");
}
