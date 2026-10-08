use super::*;

/// Real `lsof -nP -iTCP -sTCP:LISTEN -F pcn` output shape from macOS: a
/// python http.server on 8000 (IPv4 and IPv6), Vite on 5173 bound to
/// `[::1]` only, Postgres on 5432 on all interfaces, and a server bound to a
/// LAN address.
const FIXTURE: &str = "p4211\ncPython\nf3\nn*:8000\nf4\nn[::]:8000\n\
p5120\ncnode\nf23\nn[::1]:5173\n\
p97\ncpostgres\nf7\nn127.0.0.1:5432\nf8\nn[::1]:5432\n\
p300\ncrapportd\nf5\nn192.168.1.20:49152\n";

#[test]
fn lsof_fields_are_parsed_per_process() {
    let listeners = parse_lsof(FIXTURE);
    assert_eq!(listeners.len(), 6);
    assert_eq!(
        listeners[0],
        Listener {
            pid: 4211,
            process: "Python".into(),
            address: "*".into(),
            port: 8000,
        }
    );
    assert_eq!(listeners[2].address, "[::1]");
    assert_eq!(listeners[2].process, "node");
    assert_eq!(listeners[5].address, "192.168.1.20");
}

#[test]
fn lsof_junk_is_skipped_not_guessed() {
    let listeners =
        parse_lsof("p12\ncx\nnnoport\nn*:notaport\nn*:0\n\nq?\nn*:9000\npbad\nn*:9001\n");
    assert_eq!(listeners.len(), 1);
    assert_eq!(listeners[0].port, 9000);
    assert!(parse_lsof("").is_empty());
}

#[test]
fn only_loopback_reachable_listeners_survive_one_per_port() {
    let listeners = dedupe_by_port(parse_lsof(FIXTURE));
    let ports: Vec<(u16, &str)> = listeners
        .iter()
        .map(|l| (l.port, l.address.as_str()))
        .collect();
    assert_eq!(
        ports,
        vec![(5173, "[::1]"), (5432, "127.0.0.1"), (8000, "*")]
    );
}

#[test]
fn loopback_host_maps_bind_addresses_to_loopback() {
    assert_eq!(loopback_host("*"), Some("127.0.0.1"));
    assert_eq!(loopback_host("127.0.0.1"), Some("127.0.0.1"));
    assert_eq!(loopback_host("[::1]"), Some("[::1]"));
    assert_eq!(loopback_host("[::]"), Some("[::1]"));
    assert_eq!(loopback_host("192.168.1.20"), None);
    assert_eq!(loopback_host("127.0.0.2"), None);
}

#[test]
fn own_process_and_dev_frontend_are_excluded() {
    let own = Listener {
        pid: std::process::id(),
        process: "beekeeper".into(),
        address: "127.0.0.1".into(),
        port: 7777,
    };
    assert!(excluded(&own, None));
    let dev = Listener {
        pid: 1,
        process: "node".into(),
        address: "127.0.0.1".into(),
        port: 1420,
    };
    assert!(excluded(&dev, Some(1420)));
    assert!(!excluded(&dev, None));
}

/// The listing is the whole scan: a listener it names is never contacted.
/// A real loopback listener stands in for a dev server whose first
/// connection must not be stolen (a WebSocket-upgrade test harness); after
/// building the list from an `lsof` listing that names it, it has accepted
/// nothing.
#[test]
fn listing_servers_never_connects_to_any_of_them() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.set_nonblocking(true).expect("nonblocking");
    let port = listener.local_addr().expect("addr").port();
    let raw = format!("p4242\ncflutter_tester\nf9\nn127.0.0.1:{port}\n");

    let servers = servers_from_listing(&raw, None);

    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0].port, port);
    assert_eq!(servers[0].process, "flutter_tester");
    assert_eq!(servers[0].url, format!("http://localhost:{port}/"));
    assert_eq!(servers[0].title, None, "no title: nothing was fetched");
    match listener.accept() {
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
        Ok(_) => panic!("listing the servers connected to one of them"),
        Err(error) => panic!("unexpected accept error: {error}"),
    }
}

#[test]
fn listing_keeps_every_reachable_listener_not_only_pages() {
    let servers = servers_from_listing(FIXTURE, None);
    let ports: Vec<u16> = servers.iter().map(|server| server.port).collect();
    // Postgres is listed too: without a probe a page and a database look
    // alike, and the person decides by opening one.
    assert_eq!(ports, vec![5173, 5432, 8000]);
    assert!(servers.iter().all(|server| server.title.is_none()));
}
