use tg::nw;

#[test]
fn test_ip_to_bytes() {
    let (ip, port) = nw::sockaddr_to_bytes("[::1]:8080".parse().unwrap()).unwrap();
    assert_eq!(ip.len(), 16);
    assert_eq!(port, 8080);

    let addr = nw::bytes_to_sockaddr(&ip, port).unwrap();
    assert_eq!(format!("{:?}", addr), "[::1]:8080");

    let (ip, port) = nw::sockaddr_to_bytes("192.168.0.112:6688".parse().unwrap()).unwrap();
    assert_eq!(ip.len(), 4);
    assert_eq!(port, 6688);

    let addr = nw::bytes_to_sockaddr(&ip, port).unwrap();
    assert_eq!(format!("{:?}", addr), "192.168.0.112:6688");
}
