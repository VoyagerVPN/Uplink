use super::*;
use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair};
use rustls::{ServerConfig, ServerConnection, StreamOwned, pki_types::PrivatePkcs8KeyDer};
use std::{
    cell::RefCell,
    io::{BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, TcpListener},
    sync::Arc,
};
use std::{fs, path::PathBuf};

thread_local! {
    static TEST_CA: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

pub(super) fn configure_tls(client: &mut curl::easy::Easy) -> Result<(), curl::Error> {
    TEST_CA.with_borrow(|ca| {
        if let Some(ca) = ca {
            client.noproxy("*")?;
            client.cainfo(ca)?;
            let mut options = curl::easy::SslOpt::new();
            options.revoke_best_effort(true);
            client.ssl_options(&options)?;
        }
        Ok(())
    })
}

fn test_tls() -> (Arc<ServerConfig>, tempfile::TempDir) {
    let ca_key = KeyPair::generate().unwrap();
    let mut params = CertificateParams::default();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "Uplink test root");
    let ca = params.self_signed(&ca_key).unwrap();
    let issuer = Issuer::from_params(&params, ca_key);
    let key = KeyPair::generate().unwrap();
    let cert = CertificateParams::new(vec!["127.0.0.1".into()])
        .unwrap()
        .signed_by(&key, &issuer)
        .unwrap();
    let tls = Arc::new(
        ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![cert.der().clone()],
                PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
            )
            .unwrap(),
    );
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root.pem");
    fs::write(&root, ca.pem()).unwrap();
    TEST_CA.set(Some(root));
    (tls, dir)
}

#[test]
fn https_sync_reuses_connection_and_recovers_after_server_close() {
    let (tls, _directory) = test_tls();
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let host = listener.local_addr().unwrap().to_string();
    let server = thread::spawn(move || {
        let mut connections = 0;
        let mut requests = 0;
        while requests < 6 {
            let (socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(8)))
                .unwrap();
            let tls = ServerConnection::new(tls.clone()).unwrap();
            let mut stream = BufReader::new(StreamOwned::new(tls, socket));
            connections += 1;
            loop {
                let mut line = String::new();
                if stream.read_line(&mut line).unwrap_or_default() == 0 {
                    break;
                }
                let endpoint = line.split_whitespace().nth(1).unwrap().to_owned();
                let mut size = None;
                loop {
                    line.clear();
                    assert!(stream.read_line(&mut line).unwrap() > 0);
                    if line == "\r\n" {
                        break;
                    }
                    if let Some((k, v)) = line.split_once(':') {
                        if k.eq_ignore_ascii_case("content-length") {
                            size = v.trim().parse().ok();
                        }
                    }
                }
                let mut body = vec![0; size.unwrap()];
                stream.read_exact(&mut body).unwrap();
                requests += 1;
                let body = serde_json::to_vec(&json!({
                    "path": endpoint, "body": serde_json::from_slice::<Value>(&body).unwrap()
                }))
                .unwrap();
                let connection = if requests == 4 { "close" } else { "keep-alive" };
                let reply = stream.get_mut();
                write!(
                    reply,
                    "HTTP/1.1 200 OK\r\nConnection: {connection}\r\nContent-Length: {}\r\n\r\n",
                    body.len()
                )
                .unwrap();
                reply.write_all(&body).unwrap();
                reply.flush().unwrap();
                if requests == 4 || requests == 6 {
                    break;
                }
            }
        }
        connections
    });
    for i in 0..6 {
        let endpoint = if i % 2 == 0 { "sync" } else { "ping" };
        let body = json!({"sequence": i});
        let (reply, _) = request(&host, endpoint, body.clone()).unwrap();
        assert_eq!(
            reply,
            json!({"path":format!("/v1/{endpoint}"), "body":body})
        );
    }
    TEST_CA.set(None);
    assert_eq!(server.join().unwrap(), 2);
}

#[test]
fn rejected_join_records_http_status_without_logging_either_http_body() {
    let (tls, dir) = test_tls();
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let host = listener.local_addr().unwrap().to_string();
    let service = thread::spawn(move || {
        let (socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut stream = BufReader::new(StreamOwned::new(
            ServerConnection::new(tls).unwrap(),
            socket,
        ));
        let mut lines = (&mut stream).lines().map(Result::unwrap);
        assert!(lines.next().unwrap().starts_with("POST /v1/login "));
        let n = lines
            .take_while(|l| !l.is_empty())
            .filter_map(|l| {
                l.to_ascii_lowercase()
                    .strip_prefix("content-length: ")?
                    .parse::<usize>()
                    .ok()
            })
            .last()
            .unwrap();
        let mut body = vec![0; n];
        stream.read_exact(&mut body).unwrap();
        assert!(
            String::from_utf8(body)
                .unwrap()
                .contains("private-password")
        );
        let reply = r#"{"error":"private-response"}"#;
        write!(
            stream.get_mut(),
            "HTTP/1.1 403 Forbidden\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{reply}",
            reply.len()
        )
        .unwrap();
        stream.get_mut().flush().unwrap();
    });
    let result = join_log::run(
        &dir.path().join("prefs.json"),
        "password",
        &|_| {},
        |progress| {
            progress(crate::settings::JoinStage::Access);
            post(&host, "login", json!({"password":"private-password"})).map(|_| ())
        },
    );
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    service.join().unwrap();
    let log = fs::read_to_string(dir.path().join("last-join.log")).unwrap();
    assert!(log.contains("operation=http_login http_status=403"));
    assert!(log.contains("result=failed operation=http_login"));
    assert!(!log.contains("private-"));
    TEST_CA.set(None);
}

#[test]
fn a_rejected_password_change_preserves_the_last_accepted_secret() {
    use crate::settings::{network_action, network_password};
    let (tls, dir) = test_tls();
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let host = listener.local_addr().unwrap().to_string();
    let service = thread::spawn(move || {
        for (status, payload) in [
            (200, r#"{"password_revision":"v1"}"#),
            (403, "{}"),
            (200, "{}"),
        ] {
            let (socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut stream = BufReader::new(StreamOwned::new(
                ServerConnection::new(tls.clone()).unwrap(),
                socket,
            ));
            let mut lines = (&mut stream).lines();
            assert!(
                lines
                    .next()
                    .unwrap()
                    .unwrap()
                    .starts_with("POST /v1/manage ")
            );
            let size = lines
                .map(Result::unwrap)
                .take_while(|l| !l.is_empty())
                .filter_map(|line| {
                    line.to_lowercase()
                        .strip_prefix("content-length: ")
                        .and_then(|v| v.parse::<usize>().ok())
                })
                .last()
                .unwrap();
            stream.read_exact(&mut vec![0; size]).unwrap();
            write!(stream.get_mut(),"HTTP/1.1 {status} Result\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",payload.len()).unwrap();
            stream.get_mut().flush().unwrap();
        }
    });
    let path = dir.path().join("prefs.json");
    let url = format!("uplink://{host}/networks/abcd");
    let live = json!({"id":"abcd","transport":"local-fixture","host":host,"name":"Lab","node":"owner","session":"fixture",
        "rows":[{"node":"owner","nick":"Owner","role":"owner","present":true,"ip":"10.0.0.1"}],
        "methods":{"link":true,"qr":false,"password":false}});
    fs::write(
        &path,
        json!({"power":false,"rooms":[&url],"live":{&url:live}}).to_string(),
    )
    .unwrap();
    network_action(&path, &url, "password", "first fixture").unwrap();
    assert_eq!(network_password(&path, &url).unwrap(), "first fixture");
    assert!(network_action(&path, &url, "password", "rejected fixture").is_err());
    assert_eq!(network_password(&path, &url).unwrap(), "first fixture");
    network_action(&path, &url, "password-mode", "").unwrap();
    assert!(network_password(&path, &url).unwrap().is_empty());
    TEST_CA.set(None);
    service.join().unwrap();
}

#[test]
#[ignore = "requires an installed QA service and a local HTTPS EasyTier instance"]
fn joins_local_secure_network_and_receives_udp() {
    use std::net::UdpSocket;
    let path = crate::settings::path().unwrap();
    let ticket = fs::read_to_string(std::env::var_os("UPLINK_QA_TICKET").unwrap()).unwrap();
    TEST_CA.set(Some(PathBuf::from(
        std::env::var_os("UPLINK_QA_CA").unwrap(),
    )));
    crate::settings::join_headless(
        &path,
        &format!("uplink://127.0.0.1/invite/{}", ticket.trim()),
    )
    .unwrap();
    let room = crate::settings::rooms(&path).unwrap().remove(0);
    let live = room.live.unwrap();
    let mut octets = live.own_ip.parse::<Ipv4Addr>().unwrap().octets();
    octets[3] = 1;
    let target = match std::env::var_os("UPLINK_QA_ECHO_FILE") {
        Some(file) => fs::read_to_string(file).unwrap().trim().parse().unwrap(),
        None => Ipv4Addr::from(octets),
    };
    let socket = UdpSocket::bind((live.own_ip.parse::<Ipv4Addr>().unwrap(), 0)).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_millis(350)))
        .unwrap();
    socket.connect((target, 39444)).unwrap();
    let nonce = uuid::Uuid::new_v4();
    let mut reply = [0u8; 32];
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        socket.send(nonce.as_bytes()).unwrap();
        if socket
            .recv(&mut reply)
            .is_ok_and(|n| &reply[..n] == nonce.as_bytes())
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "No UDP reply from the self-hosted peer"
        );
        thread::sleep(Duration::from_millis(300));
    }
    if std::env::var_os("UPLINK_QA_SHORT_CREDENTIAL").is_some() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = || SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let expiry = serde_json::to_value(&live).unwrap()["credential_expires_at"].as_u64()
            .expect("save the engine expiry when joining");
        let remaining = expiry.saturating_sub(now());
        assert!(remaining <= 30, "fixture must issue a short first credential");
        drop(socket);
        crate::settings::set_option(&path, "power", false).unwrap();
        thread::sleep(Duration::from_secs(remaining + 1));
        crate::settings::set_option(&path, "power", true).unwrap();
        crate::settings::refresh_transport(&path).unwrap();
        let restored = crate::settings::rooms(&path).unwrap().remove(0).live.unwrap();
        assert_eq!(restored.session, live.session);
        assert_eq!(restored.node, live.node);
        assert_eq!(restored.assigned_ip, live.assigned_ip);
        assert!(serde_json::to_value(&restored).unwrap()["credential_expires_at"].as_u64().unwrap() > expiry);
        let until = Instant::now() + Duration::from_secs(20);
        loop {
            let echoed = (|| -> io::Result<bool> {
                let socket = UdpSocket::bind((live.own_ip.parse::<Ipv4Addr>().unwrap(), 0))?;
                socket.set_read_timeout(Some(Duration::from_millis(350)))?;
                socket.connect((target, 39444))?;
                let nonce = uuid::Uuid::new_v4();
                socket.send(nonce.as_bytes())?;
                let n = socket.recv(&mut reply)?;
                Ok(&reply[..n] == nonce.as_bytes())
            })();
            if echoed.unwrap_or(false) {break}
            assert!(Instant::now()<until, "No UDP echo after the original credential expired");
            thread::sleep(Duration::from_millis(250));
        }
    }
    crate::settings::leave(&path, Some(&live.key())).unwrap();
}

#[test]
#[ignore = "requires two isolated HTTPS instances and the installed QA network service"]
fn exercise_local_invitation() {
    use crate::settings;
    let path = settings::path().unwrap();
    let host = std::env::var("UPLINK_QA_HOST").unwrap();
    assert!(matches!(host.as_str(), "127.0.0.1" | "127.0.0.2"));
    TEST_CA.set(Some(PathBuf::from(
        std::env::var_os("UPLINK_QA_CA").unwrap(),
    )));
    match std::env::var("UPLINK_QA_ACTION").unwrap().as_str() {
        "join" => {
            let ticket = fs::read_to_string(std::env::var_os("UPLINK_QA_TICKET").unwrap()).unwrap();
            settings::join_headless(&path, &format!("uplink://{host}/invite/{}", ticket.trim()))
                .unwrap();
        }
        "leave" => {
            let live = settings::rooms(&path)
                .unwrap()
                .into_iter()
                .filter_map(|r| r.live)
                .find(|live| live.host == host)
                .unwrap();
            settings::leave(&path, Some(&live.key())).unwrap();
        }
        _ => panic!("Unexpected QA action"),
    }
}

#[test]
#[ignore = "requires local Uplink HTTPS avatar test instance"]
fn a_second_windows_profile_receives_and_replaces_the_owners_avatar() {
    let dir = PathBuf::from(std::env::var_os("UPLINK_AVATAR_QA").unwrap());
    let fixture: Value =
        serde_json::from_slice(&fs::read(dir.join("fixture.json")).unwrap()).unwrap();
    TEST_CA.set(Some(dir.join("ca.pem")));
    let profile = |who: &str| {
        let live: Membership = serde_json::from_value(json!({
            "host":fixture["host"],"id":fixture["group"],"name":"Avatar QA",
            "node":fixture[who]["node"],"session":fixture[who]["session"],
            "credential_expires_at":fixture[who]["expiry"]
        }))
        .unwrap();
        let path = dir.join(who).join("Uplink/prefs.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            json!({"rooms":[live.key()],"power":true,"live":{(live.key()):live}}).to_string(),
        )
        .unwrap();
        path
    };
    let owner = profile("owner");
    let friend = profile("friend");
    crate::settings::refresh_transport(&owner).unwrap();
    for (turn, color) in [[90, 180, 32, 255], [42, 79, 208, 255]]
        .into_iter()
        .enumerate()
    {
        let mut pixels = color.repeat(512 * 512);
        // Несжимаемая часть проверяет загрузку через обычный HTTPS с реальным размером фото.
        let mut seed = 42u32;
        for byte in &mut pixels[..512 * 256 * 4] {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            *byte = seed as u8;
        }
        let payload = crate::network_avatar::encode(&pixels).unwrap();
        let network = crate::settings::rooms(&owner).unwrap().remove(0).url;
        crate::settings::network_action(&owner, &network, "avatar", &payload).unwrap();
        crate::settings::refresh_transport(&friend).unwrap();
        for path in [&owner, &friend] {
            let live = crate::settings::rooms(path)
                .unwrap()
                .remove(0)
                .live
                .unwrap();
            let file = live.avatar_file(path).unwrap();
            let bitmap = image::open(file).unwrap().into_rgba8();
            assert_eq!(bitmap.dimensions(), (512, 512));
            assert_eq!(bitmap.into_raw(), pixels);
            let reply=post(&live.host,"sync",json!({"session":live.session,"nick":"QA","avatar_revision":live.known_avatar(path)})).unwrap();
            assert!(reply.get("avatar").is_none());
        }
        let ready = dir.join(format!("ready-{turn}"));
        fs::write(ready, b"").unwrap();
        if std::env::var_os("UPLINK_AVATAR_VIEW").is_some() {
            let deadline = Instant::now() + Duration::from_secs(60);
            while !dir.join(format!("viewed-{turn}")).exists() {
                assert!(Instant::now() < deadline, "Timed out waiting for the UI");
                thread::sleep(Duration::from_millis(150));
            }
        }
    }
}
