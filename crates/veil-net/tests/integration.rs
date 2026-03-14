use std::net::SocketAddr;
use veil_crypto::Identity;
use veil_net::{create_endpoint, PeerEvent, PeerManager, WireMessage};

#[tokio::test]
async fn two_peers_connect_and_exchange_messages() {
    let addr1: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let addr2: SocketAddr = "127.0.0.1:0".parse().unwrap();

    let ep1 = create_endpoint(addr1).unwrap();
    let ep2 = create_endpoint(addr2).unwrap();

    let actual_addr2 = ep2.local_addr().unwrap();

    let id1 = Identity::generate();
    let id2 = Identity::generate();

    let peer_id1 = id1.peer_id();
    let peer_id2 = id2.peer_id();
    let id1_bytes = id1.to_bytes();
    let id2_bytes = id2.to_bytes();

    let mut pm1 = PeerManager::new(ep1, peer_id1, id1_bytes);
    let mut pm2 = PeerManager::new(ep2.clone(), peer_id2.clone(), id2_bytes);

    let mut events1 = pm1.take_event_receiver().unwrap();
    let mut events2 = pm2.take_event_receiver().unwrap();

    // Start accept loop for pm2
    let connections2 = pm2.connections_handle();
    let event_tx2 = pm2.event_sender();
    tokio::spawn(PeerManager::accept_loop(
        ep2,
        peer_id2,
        id2_bytes,
        event_tx2,
        connections2,
        None,
    ));

    // pm1 connects to pm2 (includes challenge-response + DH key exchange)
    let conn_id1 = pm1.connect(actual_addr2).await.unwrap();

    // pm1 should get Connected event with session_key
    let event = events1.recv().await.unwrap();
    let session_key_1 = match &event {
        PeerEvent::Connected { session_key, .. } => *session_key,
        _ => panic!("expected Connected event"),
    };

    // pm2 should also get Connected event from accept_loop with session_key
    let event = events2.recv().await.unwrap();
    let (conn_id2, session_key_2) = match &event {
        PeerEvent::Connected {
            conn_id,
            session_key,
            ..
        } => (*conn_id, *session_key),
        _ => panic!("expected Connected event"),
    };

    // Both sides should derive the same session key
    assert_eq!(session_key_1, session_key_2);

    // pm1 sends Ping to pm2
    pm1.send_to(conn_id1, &WireMessage::Ping(42))
        .await
        .unwrap();

    // pm2 should receive the Ping
    let event = events2.recv().await.unwrap();
    match event {
        PeerEvent::Message {
            message: WireMessage::Ping(v),
            ..
        } => assert_eq!(v, 42),
        _ => panic!("expected Ping message, got {event:?}"),
    }

    // pm2 sends Pong back via shared connections map
    pm2.send_to(conn_id2, &WireMessage::Pong(42))
        .await
        .unwrap();

    // pm1 should receive the Pong
    let event = events1.recv().await.unwrap();
    match event {
        PeerEvent::Message {
            message: WireMessage::Pong(v),
            ..
        } => assert_eq!(v, 42),
        _ => panic!("expected Pong message, got {event:?}"),
    }
}

#[tokio::test]
async fn challenge_response_rejects_wrong_identity() {
    // This test verifies that a peer cannot impersonate another identity.
    // We create a scenario where a MITM-like peer claims a different identity.
    let addr1: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let addr2: SocketAddr = "127.0.0.1:0".parse().unwrap();

    let ep1 = create_endpoint(addr1).unwrap();
    let ep2 = create_endpoint(addr2).unwrap();

    let actual_addr2 = ep2.local_addr().unwrap();

    let id1 = Identity::generate();
    let id2 = Identity::generate();
    let fake_id = Identity::generate();

    let peer_id1 = id1.peer_id();
    // Peer 2 claims to be fake_id but actually has id2's signing key
    let fake_peer_id = fake_id.peer_id();
    let id1_bytes = id1.to_bytes();
    let id2_bytes = id2.to_bytes();

    let mut pm1 = PeerManager::new(ep1, peer_id1, id1_bytes);

    // pm2 uses the wrong peer_id (fake) but real signing key (id2)
    // This should cause authentication to fail because id2 can't sign for fake_id
    let mut pm2 = PeerManager::new(ep2.clone(), fake_peer_id.clone(), id2_bytes);

    let _events1 = pm1.take_event_receiver().unwrap();
    let _events2 = pm2.take_event_receiver().unwrap();

    let connections2 = pm2.connections_handle();
    let event_tx2 = pm2.event_sender();
    tokio::spawn(PeerManager::accept_loop(
        ep2,
        fake_peer_id,
        id2_bytes,
        event_tx2,
        connections2,
        None,
    ));

    // The connect should fail because the responder signs with id2's key
    // but claims to be fake_id — the signature verification will fail
    let result = pm1.connect(actual_addr2).await;
    assert!(result.is_err(), "connection should fail with mismatched identity");
}
