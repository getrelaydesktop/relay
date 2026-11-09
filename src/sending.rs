//! Optimistic messages keep their model row while Telegram assigns a server ID.
use crate::MessageRow;
use slint::{Model, VecModel};

pub struct PendingSend {
    pub peer_id: i32,
    pub local_id: i32,
}

pub fn acknowledge(rows: &VecModel<MessageRow>, local_id: i32, mut sent: MessageRow) {
    if let Some(local) = rows.iter().find(|row| row.id == local_id) {
        // An update can arrive before the send response. Remove that exact
        // server ID, then replace the optimistic row without replaying it.
        if let Some(duplicate) = rows.iter().position(|row| row.id == sent.id) {
            rows.remove(duplicate);
        }
        if let Some(index) = rows.iter().position(|row| row.id == local_id) {
            sent.send_token = local.send_token;
            rows.set_row_data(index, sent);
        }
    }
}

pub fn merge_history(rows: &VecModel<MessageRow>, mut history: Vec<MessageRow>) {
    history.extend(rows.iter().filter(|row| row.pending || row.failed));
    let retained: std::collections::HashSet<_> = history.iter().map(|row| row.id).collect();
    for index in (0..rows.row_count()).rev() {
        if !retained.contains(&rows.row_data(index).unwrap().id) {
            rows.remove(index);
        }
    }
    // Keep delegates for optimistic rows alive while a history response fills
    // in earlier messages. Replacing the entire model would cut off a flight.
    for (index, row) in history.iter_mut().enumerate() {
        if let Some(old) = rows.row_data(index).filter(|old| old.id == row.id) {
            row.send_token = old.send_token;
            if old != *row {
                rows.set_row_data(index, row.clone());
            }
        } else {
            rows.insert(index, row.clone());
        }
    }
    while rows.row_count() > history.len() {
        rows.remove(history.len());
    }
}

pub fn fail(rows: &VecModel<MessageRow>, local_id: i32) {
    if let Some(index) = rows.iter().position(|row| row.id == local_id) {
        let mut row = rows.row_data(index).unwrap();
        row.pending = false;
        row.failed = true;
        rows.set_row_data(index, row);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local() -> MessageRow {
        MessageRow {
            id: -7,
            send_token: -7,
            body: "draft survives".into(),
            pending: true,
            ..Default::default()
        }
    }

    #[test]
    fn server_echo_and_ack_leave_one_row_with_the_same_flight_token() {
        let server = MessageRow {
            id: 123,
            body: "draft survives".into(),
            ..Default::default()
        };
        let rows = VecModel::from(vec![local(), server.clone()]);
        acknowledge(&rows, -7, server);
        assert_eq!(rows.row_count(), 1);
        let row = rows.row_data(0).unwrap();
        assert_eq!(row.id, 123);
        assert_eq!(row.send_token, -7);
        assert!(!row.pending && !row.failed);
    }

    #[test]
    fn history_and_failure_keep_the_unsent_text_available_for_retry() {
        let rows = VecModel::from(vec![local()]);
        merge_history(&rows, vec![]);
        assert_eq!(rows.row_count(), 1);
        fail(&rows, -7);
        merge_history(&rows, vec![]);
        let row = rows.row_data(0).unwrap();
        assert_eq!(row.body, "draft survives");
        assert!(row.failed && !row.pending);
    }
}
