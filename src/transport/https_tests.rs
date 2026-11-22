use super::*;
use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair};
use rustls::{ServerConfig, ServerConnection, StreamOwned, pki_types::PrivatePkcs8KeyDer};
use std::{
    cell::RefCell,
    io::{BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, TcpListener},
    sync::Arc,
};

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

#[test]
fn https_sync_reuses_connection_and_recovers_after_server_close() {
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
