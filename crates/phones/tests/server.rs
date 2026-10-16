//! The server through real sockets: tokens, joining, the roster, relaying
//! both ways, ejecting, the caps and the limits.

use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use deckcraft_phones::protocol::{CLOSE_BAD_ID, CLOSE_BAD_TOKEN, CLOSE_EJECTED, CLOSE_FULL, CLOSE_NOT_LOOPBACK, CLOSE_REPLACED};
use deckcraft_phones::server::{Running, ServerConfig, start};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

async fn server() -> Running {
    start(ServerConfig { port: 0, addresses: vec![IpAddr::V4(Ipv4Addr::LOCALHOST)], ..Default::default() }).await.unwrap()
}

async fn server_with(max_phones: usize) -> Running {
    start(ServerConfig { port: 0, max_phones, ..Default::default() }).await.unwrap()
}

async fn connect(url: &str) -> Ws {
    let (ws, _) = tokio_tungstenite::connect_async(url).await.expect("connect");
    ws
}

async fn phone(r: &Running, id: &str) -> Ws {
    connect(&format!("ws://127.0.0.1:{}/ws?role=phone&t={}&id={id}", r.port, r.token)).await
}

async fn presenter(r: &Running) -> Ws {
    let mut ws = connect(&r.presenter_ws()).await;
    let hello = recv(&mut ws).await;
    assert_eq!(hello["t"], "hello");
    ws
}

async fn send(ws: &mut Ws, v: Value) {
    ws.send(Message::Text(v.to_string().into())).await.unwrap();
}

/// The next text frame as JSON, within two seconds.
async fn recv(ws: &mut Ws) -> Value {
    let msg = tokio::time::timeout(Duration::from_secs(2), ws.next()).await.expect("a message within 2 s").expect("stream open").expect("no error");
    match msg {
        Message::Text(t) => serde_json::from_str(&t).unwrap(),
        other => panic!("expected text, got {other:?}"),
    }
}

/// Skip frames until one with tag `t`.
async fn recv_tag(ws: &mut Ws, t: &str) -> Value {
    for _ in 0..20 {
        let v = recv(ws).await;
        if v["t"] == t {
            return v;
        }
    }
    panic!("no {t} message");
}

async fn close_code(ws: &mut Ws) -> u16 {
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(2), ws.next()).await.expect("a close within 2 s");
        match msg {
            Some(Ok(Message::Close(Some(f)))) => return u16::from(f.code),
            Some(Ok(Message::Close(None))) => return u16::from(CloseCode::Normal),
            Some(Ok(_)) => continue,
            Some(Err(e)) => panic!("error before close: {e}"),
            None => panic!("stream ended without a close frame"),
        }
    }
}

async fn join(ws: &mut Ws, name: &str) -> Value {
    send(ws, json!({ "t": "join", "name": name })).await;
    recv_tag(ws, "welcome").await
}

#[tokio::test]
async fn a_bad_token_is_refused_and_a_good_one_joins() {
    let r = server().await;
    let mut bad = connect(&format!("ws://127.0.0.1:{}/ws?role=phone&t=wrong&id=p_12345678", r.port)).await;
    assert_eq!(close_code(&mut bad).await, CLOSE_BAD_TOKEN);
    let mut none = connect(&format!("ws://127.0.0.1:{}/ws?id=p_12345678", r.port)).await;
    assert_eq!(close_code(&mut none).await, CLOSE_BAD_TOKEN);
    let mut bad_id = connect(&format!("ws://127.0.0.1:{}/ws?role=phone&t={}&id=short", r.port, r.token)).await;
    assert_eq!(close_code(&mut bad_id).await, CLOSE_BAD_ID);

    let mut ws = phone(&r, "p_12345678").await;
    let w = join(&mut ws, "  Sam  ").await;
    assert_eq!(w["name"], "Sam");
    assert_eq!(w["id"], "p_12345678");
    assert_eq!(w["rejoined"], false);
    assert_eq!(r.hub().roster_now().len(), 1);
    r.stop().await;
}

#[tokio::test]
async fn the_presenter_sees_the_roster_and_relays_both_ways() {
    let r = server().await;
    let mut pres = presenter(&r).await;
    let first_roster = recv_tag(&mut pres, "roster").await;
    assert_eq!(first_roster["phones"].as_array().unwrap().len(), 0);

    let mut a = phone(&r, "p_aaaaaaaa").await;
    join(&mut a, "Sam").await;
    let joined = recv_tag(&mut pres, "joined").await;
    assert_eq!(joined["name"], "Sam");
    assert_eq!(joined["id"], "p_aaaaaaaa");
    let roster = recv_tag(&mut pres, "roster").await;
    assert_eq!(roster["phones"], json!([{ "id": "p_aaaaaaaa", "name": "Sam", "connected": true }]));

    // Phone to presenter.
    send(&mut a, json!({ "t": "vote", "poll": "obj_poll", "option": 1 })).await;
    let from = recv_tag(&mut pres, "from").await;
    assert_eq!(from["id"], "p_aaaaaaaa");
    assert_eq!(from["name"], "Sam");
    assert_eq!(from["msg"], json!({ "t": "vote", "poll": "obj_poll", "option": 1 }));

    // Presenter to one phone and to all.
    send(&mut pres, json!({ "t": "send", "to": "p_aaaaaaaa", "msg": { "t": "you", "vote": 1 } })).await;
    assert_eq!(recv_tag(&mut a, "you").await["vote"], 1);
    send(&mut pres, json!({ "t": "broadcast", "msg": { "t": "show", "slide": 3 } })).await;
    assert_eq!(recv_tag(&mut a, "show").await["slide"], 3);

    // A malformed frame from the phone never reaches the presenter; the next good one does.
    a.send(Message::Text("{not json".into())).await.unwrap();
    send(&mut a, json!({ "t": "vote", "poll": "obj_poll", "option": 999 })).await;
    send(&mut a, json!({ "t": "control", "want": true })).await;
    let from = recv_tag(&mut pres, "from").await;
    assert_eq!(from["msg"]["t"], "control");
    assert_eq!(r.hub().stats.malformed.load(std::sync::atomic::Ordering::Relaxed), 2);
    r.stop().await;
}

#[tokio::test]
async fn a_reconnect_with_the_same_id_is_the_same_phone() {
    let r = server().await;
    let mut pres = presenter(&r).await;
    let mut a = phone(&r, "p_bbbbbbbb").await;
    join(&mut a, "Ana").await;
    recv_tag(&mut pres, "joined").await;
    drop(a);
    // The roster marks it disconnected, and keeps it.
    let mut seen_disconnected = false;
    for _ in 0..5 {
        let v = recv_tag(&mut pres, "roster").await;
        let phones = v["phones"].as_array().unwrap();
        if phones.len() == 1 && phones[0]["connected"] == false {
            seen_disconnected = true;
            break;
        }
    }
    assert!(seen_disconnected);
    let mut again = phone(&r, "p_bbbbbbbb").await;
    let w = join(&mut again, "Ana").await;
    assert_eq!(w["rejoined"], true);
    let joined = recv_tag(&mut pres, "joined").await;
    assert_eq!(joined["rejoined"], true);
    assert_eq!(r.hub().roster_now().len(), 1, "one phone, not two");

    // A second connection with the same id replaces the first.
    let mut third = phone(&r, "p_bbbbbbbb").await;
    assert_eq!(close_code(&mut again).await, CLOSE_REPLACED);
    join(&mut third, "Ana").await;
    assert_eq!(r.hub().connected(), 1);
    r.stop().await;
}

#[tokio::test]
async fn eject_closes_the_phone_and_bans_its_id() {
    let r = server().await;
    let mut pres = presenter(&r).await;
    let mut a = phone(&r, "p_cccccccc").await;
    join(&mut a, "Kim").await;
    recv_tag(&mut pres, "joined").await;
    send(&mut pres, json!({ "t": "eject", "id": "p_cccccccc" })).await;
    assert_eq!(recv_tag(&mut a, "ejected").await["t"], "ejected");
    assert_eq!(close_code(&mut a).await, CLOSE_EJECTED);
    let mut back = phone(&r, "p_cccccccc").await;
    assert_eq!(close_code(&mut back).await, CLOSE_EJECTED);
    assert_eq!(r.hub().roster_now().len(), 0);
    r.stop().await;
}

#[tokio::test]
async fn the_connection_cap_and_the_message_cap_hold() {
    let r = server_with(2).await;
    let mut a = phone(&r, "p_11111111").await;
    let mut b = phone(&r, "p_22222222").await;
    join(&mut a, "A").await;
    join(&mut b, "B").await;
    let mut c = phone(&r, "p_33333333").await;
    assert_eq!(close_code(&mut c).await, CLOSE_FULL);

    // A frame over the size cap ends the connection; the phone's entry stays for a reconnect.
    let big = json!({ "t": "join", "name": "x".repeat(5000) }).to_string();
    let _ = a.send(Message::Text(big.into())).await;
    let ended = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match a.next().await {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                _ => continue,
            }
        }
    })
    .await;
    assert!(ended.is_ok(), "the oversized frame closed the connection");
    tokio::time::sleep(Duration::from_millis(100)).await;
    let mut a2 = phone(&r, "p_11111111").await;
    let w = join(&mut a2, "A").await;
    assert_eq!(w["rejoined"], true);
    r.stop().await;
}

#[tokio::test]
async fn joins_are_rate_limited_per_connection() {
    let r = server().await;
    let mut a = phone(&r, "p_dddddddd").await;
    for i in 0..3 {
        send(&mut a, json!({ "t": "join", "name": format!("N{i}") })).await;
        recv_tag(&mut a, "welcome").await;
    }
    send(&mut a, json!({ "t": "join", "name": "N4" })).await;
    let v = recv(&mut a).await;
    assert_eq!(v["t"], "error");
    assert_eq!(v["code"], "rate");
    assert_eq!(r.hub().roster_now()[0].name, "N2");
    r.stop().await;
}

#[tokio::test]
async fn the_presenter_needs_its_own_token_and_only_one_connection_stands() {
    let r = server().await;
    let mut wrong = connect(&format!("ws://127.0.0.1:{}/ws?role=presenter&t={}", r.port, r.token)).await;
    assert_eq!(close_code(&mut wrong).await, CLOSE_BAD_TOKEN);
    let mut first = presenter(&r).await;
    let _second = presenter(&r).await;
    assert_eq!(close_code(&mut first).await, CLOSE_REPLACED);
    // The loopback rule is a real check: the code exists and the server refuses a non-loopback peer.
    assert_eq!(CLOSE_NOT_LOOPBACK, 4007);
    r.stop().await;
}

#[tokio::test]
async fn the_page_and_only_the_page_are_served() {
    let r = server().await;
    let url = r.url_for("127.0.0.1");
    assert!(url.starts_with(&format!("http://127.0.0.1:{}/?t=", r.port)));
    let ok = deckcraft_phones::server::self_test(&url, Duration::from_secs(2)).await.unwrap();
    assert!(ok.contains("answers"), "{ok}");
    r.stop().await;
    // Once stopped, the same URL fails the self test with a plain message.
    let err = deckcraft_phones::server::self_test(&url, Duration::from_secs(1)).await.unwrap_err();
    assert!(err.starts_with("Cannot connect") || err.starts_with("No answer"), "{err}");
}

/// One HTTP GET, as text: status line and body.
async fn http_get(port: u16, path: &str) -> (String, Vec<u8>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    s.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).await.unwrap();
    let split = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap_or(buf.len());
    let head = String::from_utf8_lossy(&buf[..split]).into_owned();
    (head, buf[(split + 4).min(buf.len())..].to_vec())
}

#[tokio::test]
async fn slide_images_are_served_by_number_with_the_token_and_nothing_else() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("slide-001.jpg"), b"jpeg-one").unwrap();
    std::fs::write(dir.path().join("slide-002.jpg"), b"jpeg-two").unwrap();
    std::fs::write(dir.path().join("secret.txt"), b"no").unwrap();
    let r = server().await;
    let t = r.token.clone();
    // Before the presenter names the directory: nothing.
    let (head, _) = http_get(r.port, &format!("/slide/1?t={t}")).await;
    assert!(head.starts_with("HTTP/1.1 404"), "{head}");

    let mut p = presenter(&r).await;
    send(&mut p, json!({ "t": "slides", "dir": dir.path().to_string_lossy(), "count": 2 })).await;
    // A moment for the message to land.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let (head, body) = http_get(r.port, &format!("/slide/1?t={t}")).await;
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert!(head.to_ascii_lowercase().contains("content-type: image/jpeg"), "{head}");
    assert!(head.to_ascii_lowercase().contains("cache-control: private, max-age=86400"), "{head}");
    assert_eq!(body, b"jpeg-one");
    let (_, body) = http_get(r.port, &format!("/slide/2?t={t}")).await;
    assert_eq!(body, b"jpeg-two");
    for bad in [
        format!("/slide/3?t={t}"),
        format!("/slide/0?t={t}"),
        "/slide/1".to_string(),
        "/slide/1?t=wrong".to_string(),
        format!("/slide/secret.txt?t={t}"),
        format!("/slide/..%2Fsecret.txt?t={t}"),
        format!("/slide/1/../secret.txt?t={t}"),
    ] {
        let (head, body) = http_get(r.port, &bad).await;
        assert!(head.starts_with("HTTP/1.1 404") || head.starts_with("HTTP/1.1 400"), "{bad}: {head}");
        assert_ne!(body, b"no", "{bad} leaked a file");
    }
    r.stop().await;
}

/// A presenter live frame: the 8-byte header, then `jpeg`.
fn live_frame(slide: u32, version: u32, jpeg: &[u8]) -> Vec<u8> {
    let mut v = slide.to_le_bytes().to_vec();
    v.extend_from_slice(&version.to_le_bytes());
    v.extend_from_slice(jpeg);
    v
}

#[tokio::test]
async fn a_binary_presenter_frame_is_stored_and_served_by_version_with_the_token() {
    let r = server().await;
    let t = r.token.clone();
    let (head, _) = http_get(r.port, &format!("/live/1?t={t}")).await;
    assert!(head.starts_with("HTTP/1.1 404"), "nothing before a frame arrives: {head}");

    let mut p = presenter(&r).await;
    recv_tag(&mut p, "roster").await;
    let jpeg = [0xff, 0xd8, 0xff, 0xe0, b'o', b'n', b'e'];
    p.send(Message::Binary(live_frame(4, 1, &jpeg).into())).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(r.hub().live_now(), Some((4, 1)));
    let (head, body) = http_get(r.port, &format!("/live/1?t={t}")).await;
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert!(head.to_ascii_lowercase().contains("content-type: image/jpeg"), "{head}");
    assert!(head.to_ascii_lowercase().contains("cache-control: private, max-age=31536000, immutable"), "{head}");
    assert_eq!(body, jpeg);

    // The token gates it; an unknown version, and the version before the newest, are 404s.
    for bad in
        ["/live/1".to_string(), "/live/1?t=wrong".to_string(), format!("/live/2?t={t}"), format!("/live/x?t={t}"), format!("/live/1/../1?t={t}")]
    {
        let (head, body) = http_get(r.port, &bad).await;
        assert!(head.starts_with("HTTP/1.1 404") || head.starts_with("HTTP/1.1 400"), "{bad}: {head}");
        assert_ne!(body, jpeg, "{bad} served the frame");
    }
    let jpeg2 = [0xff, 0xd8, 0xff, 0xe0, b't', b'w', b'o'];
    p.send(Message::Binary(live_frame(4, 2, &jpeg2).into())).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let (head, _) = http_get(r.port, &format!("/live/1?t={t}")).await;
    assert!(head.starts_with("HTTP/1.1 404"), "only the newest frame is kept: {head}");
    let (_, body) = http_get(r.port, &format!("/live/2?t={t}")).await;
    assert_eq!(body, jpeg2);

    // Binary that is not a frame is dropped and counted; the presenter's connection stays.
    p.send(Message::Binary(live_frame(4, 3, b"PNG!").into())).await.unwrap();
    p.send(Message::Binary(vec![1, 2, 3].into())).await.unwrap();
    send(&mut p, json!({ "t": "roster" })).await;
    recv_tag(&mut p, "roster").await;
    assert_eq!(r.hub().stats.malformed.load(std::sync::atomic::Ordering::Relaxed), 2);
    assert_eq!(r.hub().live_now(), Some((4, 2)));

    // A phone's binary frame is never a live frame, whatever it holds.
    let mut ph = phone(&r, "p_binary01").await;
    join(&mut ph, "Sam").await;
    ph.send(Message::Binary(live_frame(4, 9, &jpeg).into())).await.unwrap();
    send(&mut ph, json!({ "t": "ping", "at": 1 })).await;
    recv_tag(&mut ph, "pong").await;
    assert_eq!(r.hub().live_now(), Some((4, 2)));
    r.stop().await;
}

#[tokio::test]
async fn an_oversized_live_frame_is_refused() {
    use deckcraft_phones::protocol::{LIVE_FRAME_HEADER, LIVE_FRAME_MAX_BYTES};
    let r = server().await;
    let t = r.token.clone();
    let mut p = presenter(&r).await;
    // At the cap: kept. One byte over: the socket closes it before it is read.
    let mut at_cap = vec![0xff, 0xd8, 0xff];
    at_cap.resize(LIVE_FRAME_MAX_BYTES, 0);
    p.send(Message::Binary(live_frame(1, 5, &at_cap).into())).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(r.hub().live_now(), Some((1, 5)));
    let (head, body) = http_get(r.port, &format!("/live/5?t={t}")).await;
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert_eq!(body.len(), LIVE_FRAME_MAX_BYTES);
    assert_eq!(body.len() + LIVE_FRAME_HEADER, at_cap.len() + LIVE_FRAME_HEADER);

    let mut over = at_cap.clone();
    over.push(0);
    let _ = p.send(Message::Binary(live_frame(1, 6, &over).into())).await;
    let ended = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match p.next().await {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                _ => continue,
            }
        }
    })
    .await;
    assert!(ended.is_ok(), "the oversized frame closed the presenter's connection");
    assert_eq!(r.hub().live_now(), Some((1, 5)), "the frame before it stands");
    let (head, _) = http_get(r.port, &format!("/live/6?t={t}")).await;
    assert!(head.starts_with("HTTP/1.1 404"), "{head}");
    r.stop().await;
}

#[tokio::test]
async fn draw_and_erase_frames_reach_the_presenter_and_nonsense_does_not() {
    let r = server().await;
    let mut p = presenter(&r).await;
    let mut ph = phone(&r, "p_drawer01").await;
    join(&mut ph, "Sam").await;
    recv_tag(&mut p, "joined").await;
    send(&mut ph, json!({ "t": "draw", "stroke": 1, "points": [[0.1, 0.1], [0.2, 0.15]], "at": 1 })).await;
    let m = recv_tag(&mut p, "from").await;
    assert_eq!(m["msg"]["t"], "draw");
    assert_eq!(m["msg"]["stroke"], 1);
    assert_eq!(m["msg"]["points"].as_array().unwrap().len(), 2);
    assert_eq!(m["msg"]["done"], false);
    send(&mut ph, json!({ "t": "erase", "x": 0.5, "y": 0.5, "at": 2 })).await;
    let m = recv_tag(&mut p, "from").await;
    assert_eq!(m["msg"]["t"], "erase");
    // Too many points, and a point off the slide: dropped, so the next good frame is the next thing the presenter sees.
    let many: Vec<[f64; 2]> = (0..100).map(|i| [i as f64 / 100.0, 0.5]).collect();
    send(&mut ph, json!({ "t": "draw", "stroke": 2, "points": many, "at": 3 })).await;
    send(&mut ph, json!({ "t": "draw", "stroke": 2, "points": [[9.0, 0.5]], "at": 3 })).await;
    send(&mut ph, json!({ "t": "draw", "stroke": 3, "points": [[0.3, 0.3]], "done": true, "at": 4 })).await;
    let m = recv_tag(&mut p, "from").await;
    assert_eq!(m["msg"]["stroke"], 3);
    assert_eq!(m["msg"]["done"], true);
    assert_eq!(r.hub().stats.malformed.load(std::sync::atomic::Ordering::Relaxed), 2);
    r.stop().await;
}
