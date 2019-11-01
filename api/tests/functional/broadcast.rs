use bigneon_db::models::enums::{BroadcastChannel, BroadcastType};
use bigneon_db::prelude::Broadcast;
use std::string::ToString;
use support::database::TestDatabase;

#[test]
fn broadcast_counter() {
    let database = TestDatabase::new();
    let connection = database.connection.get();
    let id = database.create_event().finish().id;
    let broadcast = Broadcast::create(
        id,
        BroadcastType::Custom,
        BroadcastChannel::PushNotification,
        "Name".to_string(),
        Some("Custom Message".to_string()),
        None,
        None,
    )
    .commit(connection)
    .unwrap();
    Broadcast::set_sent_count(broadcast.id, 2, &connection).unwrap();
    Broadcast::increment_open_count(broadcast.id, &connection).unwrap();
    let b = Broadcast::find(broadcast.id, &connection).unwrap();
    assert_eq!(b.sent_quantity, 2);
    assert_eq!(b.opened_quantity, 1);
}
