//! Optimistic messages keep their model row while Telegram assigns a server ID.
use crate::MessageRow;
use slint::{Model, VecModel};

// Premium::CubicBezier(.25, .1, .25, 1, progress): invert the x coordinate,
// then evaluate y. Keep the original curve instead of an easing approximation.
pub fn rise_progress(progress: f32) -> f32 {
    if progress <= 0.0 {
        return 0.0;
    }
    if progress >= 1.0 {
        return 1.0;
    }
    let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
    for _ in 0..48 {
        let t = (lo + hi) / 2.0;
        let x = 0.75 * t * (1.0 - t) + t * t * t;
        if x < f64::from(progress) {
            lo = t;
        } else {
            hi = t;
        }
    }
    let t = (lo + hi) / 2.0;
    let s = 1.0 - t;
    (0.3 * s * s * t + 3.0 * s * t * t + t * t * t) as f32
}

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
            crate::details::retain(&mut sent, &local);
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
            crate::details::retain(row, &old);
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
    #[test]
    fn rise_uses_the_original_bezier() {
        assert_eq!(rise_progress(0.0), 0.0);
        assert!((rise_progress(0.5) - 0.8024034).abs() < 0.000001);
        assert_eq!(rise_progress(1.0), 1.0);
    }

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
